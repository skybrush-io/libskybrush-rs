//! Exercises the components of the crate on arbitrary input, for the fuzz
//! targets in `fuzz_targets/`.
//!
//! The targets check that the crate neither panics nor hangs on malformed
//! input; the results of the computations are not checked. Overflow checks
//! are on, since cargo-fuzz builds with debug assertions.

use std::hint::black_box;
use std::rc::Rc;

use skybrush::basic_types::Vector3;
use skybrush::control::ShowController;
use skybrush::events::{EventList, EventListPlayer, EventType};
use skybrush::formats::binary::BinaryFile;
use skybrush::formats::tlv::TlvParser;
use skybrush::gcs_light_control::GcsLightControlSetup;
use skybrush::lights::{LightPlayer, LightProgram};
use skybrush::metadata::ShowMetadata;
use skybrush::rth_plan::RthPlan;
use skybrush::screenplay::Screenplay;
use skybrush::time_axis::TimeSegment;
use skybrush::trajectory::{
    Trajectory, TrajectoryBuilder, TrajectoryPlayer, TrajectoryStatsCalculator,
};
use skybrush::yaw_control::{YawControl, YawPlayer};

/// Times in seconds at which the components are evaluated, in an order that
/// moves both forwards and backwards.
const TIMES_SEC: [f32; 13] = [
    0.0,
    0.05,
    1.0,
    10.5,
    3.0,
    100.0,
    3600.0,
    1e6,
    -1.0,
    f32::INFINITY,
    0.5,
    f32::NEG_INFINITY,
    f32::NAN,
];

/// Times in milliseconds at which light programs are evaluated. Light
/// programs are executed one command at a time, so a program of short waits
/// takes long to seek far ahead; the show controller does not seek beyond a
/// day of warped time, and the fuzzer stays well below that.
const LIGHT_TIMES_MSEC: [u64; 8] = [0, 20, 1000, 10_500, 3000, 120_000, 600_000, 5000];

/// Times of the show clock in milliseconds at which the show controller is
/// updated. The times beyond `i32::MAX` are outside every scene.
const SHOW_TIMES_MSEC: [u32; 10] = [
    0,
    100,
    1000,
    10_500,
    5000,
    120_000,
    600_000,
    (i32::MAX as u32) + 1,
    u32::MAX,
    2000,
];

pub fn trajectory(trajectory: &Trajectory<'_>) {
    let mut player = TrajectoryPlayer::new(trajectory);
    for t in TIMES_SEC {
        black_box(player.position_at(t));
        black_box(player.velocity_at(t));
        black_box(player.acceleration_at(t));
    }
    black_box(player.total_duration_msec());
    black_box(trajectory.axis_aligned_bounding_box());
    black_box(trajectory.start_position());
    black_box(trajectory.end_position());
    black_box(trajectory.propose_takeoff_time_sec(2500.0, 2000.0, 1000.0));
    black_box(trajectory.propose_landing_time_sec(2000.0, 1000.0));

    let calculator = TrajectoryStatsCalculator::new(1000.0);
    let Ok(mut stats) = calculator.run(trajectory) else {
        return;
    };

    // Modifications, on copies
    for t in TIMES_SEC {
        let mut copy = trajectory.clone();
        let _ = black_box(copy.cut_at(t));
        trajectory_player(&copy);
    }
    let mut copy = trajectory.clone();
    let target = Vector3 {
        x: 1000.0,
        y: -2000.0,
        z: 0.0,
    };
    let _ = black_box(copy.replace_end_to_land_at(&mut stats, target, 500.0));
    trajectory_player(&copy);

    let start = trajectory.start_position();
    if let Ok(mut builder) = TrajectoryBuilder::from_trajectory(trajectory.clone(), None) {
        let _ = builder.move_to_in_time(start, 5000, 1000.0);
        let _ = builder.hold_position_for(70_000);
        if let Ok(built) = builder.build() {
            trajectory_player(&built);
        }
    }
}

fn trajectory_player(trajectory: &Trajectory<'_>) {
    let mut player = TrajectoryPlayer::new(trajectory);
    for t in TIMES_SEC {
        black_box(player.position_at(t));
    }
}

pub fn light_program(program: &LightProgram<'_>) {
    let mut player = LightPlayer::new(program);
    for t in LIGHT_TIMES_MSEC {
        black_box(player.color_at(t));
        black_box(player.seek(t));
        black_box(player.next_timestamp());
    }
}

pub fn yaw_control(ctrl: &YawControl<'_>) {
    let mut player = YawPlayer::new(ctrl);
    for t in TIMES_SEC {
        black_box(player.yaw_at(t));
        black_box(player.yaw_rate_at(t));
    }
    black_box(player.total_duration_msec());
}

pub fn event_list(mut events: EventList) {
    play_events(&events);
    for duration in [0, 1000, u32::MAX] {
        let mut copy = events.clone();
        let _ = black_box(copy.add_pyro_off_events(duration));
        play_events(&copy);
    }
    for delta in [i32::MIN, -1500, 0, 2000, i32::MAX] {
        events.adjust_timestamps_by_type(EventType::PYRO, delta);
    }
    events.sort();
    play_events(&events);
}

fn play_events(events: &EventList) {
    let mut player = EventListPlayer::new(events);
    for t in TIMES_SEC {
        while let Some(event) = player.next_event_not_later_than(t) {
            black_box(event);
        }
        player.seek(t);
        black_box(player.peek_next_event());
    }
}

pub fn rth_plan(plan: &RthPlan<'_>, trajectory: Option<&Trajectory<'_>>) {
    for index in 0..=plan.num_points().min(16) {
        black_box(plan.point(index));
    }
    black_box(plan.num_entries());
    for t in TIMES_SEC {
        let Ok(entry) = plan.evaluate_at(t) else {
            continue;
        };
        let start = trajectory.map_or(Vector3::ZERO, |trajectory| {
            TrajectoryPlayer::new(trajectory)
                .position_at(entry.time_sec)
                .xyz()
        });
        if let Ok(trajectory) = Trajectory::from_rth_plan_entry(&entry, start) {
            trajectory_player(&trajectory);
        }
    }
}

pub fn tlv(body: &[u8]) {
    for entry in TlvParser::new(body) {
        let _ = black_box(entry);
    }
    if let Ok(setup) = GcsLightControlSetup::from_buffer(body) {
        let palette = setup.palette();
        for index in 0..palette.len().min(16) + 1 {
            black_box(palette.color_or_default(index));
        }
    }
    let _ = black_box(ShowMetadata::from_buffer(body));
}

/// Reads every component of a show file, and plays it with the show
/// controller.
pub fn show_file(bytes: &[u8]) {
    let Ok(file) = BinaryFile::new(bytes) else {
        return;
    };
    for block in file.blocks() {
        let _ = black_box(block);
    }

    let trajectory = Trajectory::from_binary_file(&file).ok().flatten();
    if let Some(trajectory) = &trajectory {
        self::trajectory(trajectory);
    }
    if let Ok(Some(program)) = LightProgram::from_binary_file(&file) {
        light_program(&program);
    }
    if let Ok(Some(ctrl)) = YawControl::from_binary_file(&file) {
        yaw_control(&ctrl);
    }
    if let Ok(Some(events)) = EventList::from_binary_file(&file) {
        event_list(events);
    }
    if let Ok(Some(plan)) = RthPlan::from_binary_file(&file) {
        rth_plan(&plan, trajectory.as_ref());
    }
    let _ = black_box(GcsLightControlSetup::from_binary_file(&file));
    let _ = black_box(ShowMetadata::from_binary_file(&file));

    if let Ok(screenplay) = Screenplay::from_binary_file(&file) {
        show_controller(screenplay);
    }
}

/// Plays a screenplay with the show controller, also after modifying it as
/// the firmware does: a second scene with a time axis, and a new screenplay
/// that shares the components of the first scene.
fn show_controller(screenplay: Screenplay<'_>) {
    let mut ctrl = ShowController::new(screenplay);
    update(&mut ctrl);

    let Some(first) = ctrl.screenplay().scene(0).cloned() else {
        return;
    };
    let mut screenplay = Screenplay::new();
    screenplay.set_rth_plan(ctrl.screenplay().rth_plan().map(Rc::clone));
    if let Ok(scene) = screenplay.append_new_scene() {
        scene.update_contents_from(&first);
        scene.set_duration_msec(30_000);
        let axis = scene.time_axis_mut();
        let _ = axis.append_segment(TimeSegment::realtime(10_000));
        let _ = axis.append_segment(TimeSegment::slowdown_from_realtime(5000));
        let _ = axis.append_segment(TimeSegment::constant_rate(5000, 0.0));
        let _ = axis.append_segment(TimeSegment::spinup_to_realtime(5000));
    }
    if let Ok(scene) = screenplay.append_new_scene() {
        scene.update_contents_from(&first);
        let _ = scene.set_origin_msec(30_000);
        let _ = scene
            .time_axis_mut()
            .append_segment(TimeSegment::constant_rate(10_000, 2.0));
    }
    ctrl.set_screenplay(screenplay);
    update(&mut ctrl);
}

fn update(ctrl: &mut ShowController<'_>) {
    for t in SHOW_TIMES_MSEC {
        ctrl.update_time_msec(t);
        black_box(ctrl.current_output());
        while let Some(event) = ctrl.next_event() {
            black_box(event);
        }
    }
}

/// The blocks of a show file of version 2 in a show file of version 1,
/// without the checksum, so that mutations of the blocks are not rejected for
/// a wrong checksum.
#[must_use]
pub fn as_version_1(bytes: &[u8]) -> Option<Vec<u8>> {
    let (header, rest) = bytes.split_at_checked(6)?;
    if header.get(..5)? != b"skyb\x02" {
        return None;
    }
    let features = *header.get(5)?;
    let blocks = if features & 0x01 == 0 {
        rest
    } else {
        rest.get(4..)?
    };
    let mut result = b"skyb\x01".to_vec();
    result.extend_from_slice(blocks);
    Some(result)
}
