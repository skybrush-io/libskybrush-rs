//! Checks that loading from a show file in memory and playback do not
//! allocate.
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

use common::oracle;
use skybrush::formats::binary::BinaryFile;
use skybrush::trajectory::{Trajectory, TrajectoryPlayer};

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
                std::hint::black_box(player.position_at(t));
                std::hint::black_box(player.velocity_at(t));
                std::hint::black_box(player.acceleration_at(t));
            }
            std::hint::black_box(trajectory.axis_aligned_bounding_box());
            std::hint::black_box(trajectory.end_position());
        });
        assert_eq!(count, 0, "{name}: loading or playback allocated");
    }
}

#[test]
fn the_counter_counts() {
    assert_eq!(allocations_of(|| drop(std::hint::black_box(vec![1_u8]))), 1);
}
