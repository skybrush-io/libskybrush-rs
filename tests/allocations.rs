//! Checks that loading from a show file in memory, playback and statistics
//! do not allocate.
//!
//! The heap of the module on the flight controller may not grow while the
//! drone is armed, so playback must not allocate. Reading from a show file in
//! memory borrows from it, so it does not allocate either.
#![allow(
    unsafe_code,
    reason = "a global allocator needs unsafe code; the library forbids it"
)]
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)]

mod common;

use std::alloc::{GlobalAlloc, Layout, System};
use std::cell::Cell;
use std::hint::black_box;

use common::oracle;
use skybrush::control::ShowController;
use skybrush::events::{EventList, EventListPlayer};
use skybrush::formats::binary::BinaryFile;
use skybrush::gcs_light_control::GcsLightControlSetup;
use skybrush::lights::{LightPlayer, LightProgram};
use skybrush::metadata::ShowMetadata;
use skybrush::rth_plan::RthPlan;
use skybrush::screenplay::Screenplay;
use skybrush::time_axis::TimeSegment;
use skybrush::trajectory::{Trajectory, TrajectoryPlayer, TrajectoryStatsCalculator};
use skybrush::yaw_control::{YawControl, YawPlayer};

/// Counts the allocations of each thread, since tests run in parallel.
struct CountingAllocator;

thread_local! {
    static ALLOCATIONS: Cell<usize> = const { Cell::new(0) };
}

// SAFETY: the system allocator does the work; this only counts.
unsafe impl GlobalAlloc for CountingAllocator {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        ALLOCATIONS.with(|count| count.set(count.get() + 1));
        // SAFETY: forwarded with the same layout
        unsafe { System.alloc(layout) }
    }

    unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
        // SAFETY: forwarded with the same pointer and layout
        unsafe { System.dealloc(ptr, layout) }
    }

    unsafe fn realloc(&self, ptr: *mut u8, layout: Layout, new_size: usize) -> *mut u8 {
        ALLOCATIONS.with(|count| count.set(count.get() + 1));
        // SAFETY: forwarded with the same arguments
        unsafe { System.realloc(ptr, layout, new_size) }
    }
}

#[global_allocator]
static ALLOCATOR: CountingAllocator = CountingAllocator;

/// The number of allocations made by `f` on this thread.
fn allocations_of(f: impl FnOnce()) -> usize {
    let before = ALLOCATIONS.with(Cell::get);
    f();
    ALLOCATIONS.with(Cell::get) - before
}

#[test]
fn trajectories_are_loaded_and_played_without_allocating() {
    for name in [
        "real_show",
        "multiple_vertical_landing_segments",
        "pyro_events",
    ] {
        let show = oracle::read_bytes(&format!("format/{name}.skyb"));
        let count = allocations_of(|| {
            let file = BinaryFile::new(&show).unwrap();
            let trajectory = Trajectory::from_binary_file(&file).unwrap().unwrap();
            let duration = trajectory.total_duration_sec();
            let mut player = TrajectoryPlayer::new(&trajectory);
            // Forwards, backwards and after the end
            for step in (0..=200).chain((0..=200).rev()) {
                let t = duration * f32::from(u8::try_from(step).unwrap()) / 180.0;
                black_box(player.position_at(t));
                black_box(player.velocity_at(t));
                black_box(player.acceleration_at(t));
            }
            black_box(trajectory.axis_aligned_bounding_box());
            black_box(trajectory.end_position());
            black_box(TrajectoryStatsCalculator::new(1000.0).run(&trajectory)).unwrap();
            black_box(trajectory.propose_landing_time_sec(2500.0, 50.0));
        });
        assert_eq!(
            count, 0,
            "{name}: loading, playback or statistics allocated"
        );
    }
}

#[test]
fn light_programs_are_loaded_and_played_without_allocating() {
    for name in [
        "real_show",
        "light_program_with_wait_until_cmd",
        "basic_show_with_yaw",
    ] {
        let show = oracle::read_bytes(&format!("format/{name}.skyb"));
        let count = allocations_of(|| {
            let file = BinaryFile::new(&show).unwrap();
            let program = LightProgram::from_binary_file(&file).unwrap().unwrap();
            let mut player = LightPlayer::new(&program);
            // Forwards, backwards and after the end
            for step in (0..=700).chain((0..=700).rev()) {
                black_box(player.color_at(step * 1000));
                black_box(player.next_timestamp());
            }
        });
        assert_eq!(
            count, 0,
            "{name}: loading or playing the light program allocated"
        );
    }
}

#[test]
fn yaw_control_is_loaded_and_played_without_allocating() {
    let show = oracle::read_bytes("format/basic_show_with_yaw.skyb");
    let count = allocations_of(|| {
        let file = BinaryFile::new(&show).unwrap();
        let ctrl = YawControl::from_binary_file(&file).unwrap().unwrap();
        let mut player = YawPlayer::new(&ctrl);
        // Forwards, backwards and after the end
        for step in (0..=700_u16).chain((0..=700).rev()) {
            let time_sec = f32::from(step) / 10.0;
            black_box(player.yaw_at(time_sec));
            black_box(player.yaw_rate_at(time_sec));
        }
        black_box(player.total_duration_msec());
    });
    assert_eq!(count, 0, "loading or playing the yaw control allocated");
}

#[test]
fn rth_plans_are_loaded_and_evaluated_without_allocating() {
    let show = oracle::read_bytes("format/hover_3m_with_rth_plan.skyb");
    let count = allocations_of(|| {
        let file = BinaryFile::new(&show).unwrap();
        let plan = RthPlan::from_binary_file(&file).unwrap().unwrap();
        for step in 0..=1000_u16 {
            black_box(plan.evaluate_at(f32::from(step) / 10.0).unwrap());
        }
    });
    assert_eq!(count, 0, "loading or evaluating the RTH plan allocated");
}

#[test]
fn metadata_and_light_control_setup_are_loaded_without_allocating() {
    let show = oracle::read_bytes("format/hover_3m_with_gcs_control_block.skyb");
    let count = allocations_of(|| {
        let file = BinaryFile::new(&show).unwrap();
        let setup = GcsLightControlSetup::from_binary_file(&file)
            .unwrap()
            .unwrap();
        black_box(setup.palette().color(0));
        black_box(ShowMetadata::from_binary_file(&file).unwrap());
    });
    assert_eq!(
        count, 0,
        "loading the metadata or the light control setup allocated"
    );
}

#[test]
fn events_are_played_without_allocating() {
    // The events are copied out of the show file when they are loaded, but
    // firing them must not allocate
    let show = oracle::read_bytes("format/pyro_events.skyb");
    let file = BinaryFile::new(&show).unwrap();
    let mut list = EventList::from_binary_file(&file).unwrap().unwrap();
    list.add_pyro_off_events(500).unwrap();
    let count = allocations_of(|| {
        let mut player = EventListPlayer::new(&list);
        for step in 0..=1000_u16 {
            let time_sec = f32::from(step) / 10.0;
            while let Some(event) = player.next_event_not_later_than(time_sec) {
                black_box(event);
            }
        }
        player.seek(42.0);
        black_box(player.peek_next_event());
    });
    assert_eq!(count, 0, "firing the events allocated");
}

#[test]
fn shows_are_played_without_allocating() {
    for name in ["pyro_events", "basic_show_with_yaw", "real_show"] {
        let show = oracle::read_bytes(&format!("format/{name}.skyb"));
        let file = BinaryFile::new(&show).unwrap();
        let mut screenplay = Screenplay::from_binary_file(&file).unwrap();
        // A second scene that shares the components of the first one, so
        // that the controller changes scenes
        let first = screenplay.scene(0).unwrap().clone();
        let scene = screenplay.scene_mut(0).unwrap();
        scene.set_duration_msec(30_000);
        let second = screenplay.append_new_scene().unwrap();
        second.update_contents_from(&first);
        second.set_origin_msec(30_000).unwrap();
        second
            .time_axis_mut()
            .append_segment(TimeSegment::constant_rate(10_000, 2.0))
            .unwrap();
        let mut ctrl = ShowController::new(screenplay);
        let count = allocations_of(|| {
            for step in (0..=1500_u32).chain((0..=1500).rev()) {
                ctrl.update_time_msec(step * 100);
                black_box(ctrl.current_output());
                while let Some(event) = ctrl.next_event() {
                    black_box(event);
                }
            }
        });
        assert_eq!(count, 0, "{name}: playing the show allocated");
    }
}

#[test]
fn the_counter_counts() {
    assert_eq!(allocations_of(|| drop(black_box(vec![1_u8]))), 1);
}
