//! Checks how the show controller follows modifications of its screenplay
//! during a show, which the oracle does not cover.
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)]

mod common;

use std::rc::Rc;

use common::oracle;
use skybrush::basic_types::Vector3WithYaw;
use skybrush::control::ShowController;
use skybrush::formats::binary::BinaryFile;
use skybrush::screenplay::Screenplay;
use skybrush::time_axis::TimeSegment;
use skybrush::trajectory::TrajectoryBuilder;

fn fired_events(ctrl: &mut ShowController<'_>, time_msec: u32) -> usize {
    ctrl.update_time_msec(time_msec);
    let mut count = 0;
    while ctrl.next_event().is_some() {
        count += 1;
    }
    count
}

#[test]
fn modifying_the_screenplay_does_not_fire_events_again() {
    let show = oracle::read_bytes("format/pyro_events.skyb");
    let file = BinaryFile::new(&show).unwrap();
    let mut ctrl = ShowController::new(Screenplay::from_binary_file(&file).unwrap());

    // The first event is at 10 s
    assert_eq!(fired_events(&mut ctrl, 10_000), 1);
    assert_eq!(fired_events(&mut ctrl, 10_100), 0);

    // Pause the show at 10.1 s: real time until then, then stopped time
    let axis = ctrl.screenplay_mut().scene_mut(0).unwrap().time_axis_mut();
    axis.append_segment(TimeSegment::realtime(10_100)).unwrap();
    axis.append_segment(TimeSegment::constant_rate(u32::MAX, 0.0))
        .unwrap();
    assert!(!ctrl.is_output_valid());

    assert_eq!(fired_events(&mut ctrl, 10_200), 0);
    assert!(ctrl.is_output_valid());
    let output_time = ctrl.current_output_time().expect("a valid output");
    assert!((output_time.warped_time_in_scene_sec - 10.1).abs() < 1e-3);
}

#[test]
fn replacing_a_component_of_the_current_scene_takes_effect() {
    let show = oracle::read_bytes("format/pyro_events.skyb");
    let file = BinaryFile::new(&show).unwrap();
    let mut ctrl = ShowController::new(Screenplay::from_binary_file(&file).unwrap());
    ctrl.update_time_msec(1000);
    assert_eq!(fired_events(&mut ctrl, 10_000), 1);

    let target = Vector3WithYaw {
        x: 1234.0,
        y: 0.0,
        z: 0.0,
        yaw: 0.0,
    };
    let mut builder = TrajectoryBuilder::new(1).unwrap();
    builder.set_start_position(target).unwrap();
    builder.hold_position_for(60_000).unwrap();
    let trajectory = builder.build().unwrap();
    let scene = ctrl.screenplay_mut().scene_mut(0).unwrap();
    scene.set_trajectory(Some(Rc::new(trajectory)));

    ctrl.update_time_msec(10_000);
    let position = ctrl.current_output().position().unwrap();
    assert!((position.x - 1234.0).abs() < 1e-3);
    // The events are the same, so they are not fired again
    assert_eq!(fired_events(&mut ctrl, 10_000), 0);
}

#[test]
fn replacing_the_screenplay_with_one_sharing_the_scene_keeps_the_players() {
    let show = oracle::read_bytes("format/pyro_events.skyb");
    let file = BinaryFile::new(&show).unwrap();
    let mut ctrl = ShowController::new(Screenplay::from_binary_file(&file).unwrap());
    assert_eq!(fired_events(&mut ctrl, 10_000), 1);

    // As the firmware does when a time axis configuration arrives: a new
    // screenplay whose main scene shares the components of the old one, with
    // a new time axis
    let mut screenplay = Screenplay::new();
    let scene = screenplay.append_new_scene().unwrap();
    scene.update_contents_from(ctrl.screenplay().scene(0).unwrap());
    scene
        .time_axis_mut()
        .append_segment(TimeSegment::realtime(10_500))
        .unwrap();
    scene
        .time_axis_mut()
        .append_segment(TimeSegment::constant_rate(u32::MAX, 0.0))
        .unwrap();
    let old = ctrl.set_screenplay(screenplay);
    assert_eq!(old.len(), 1);
    assert!(!ctrl.is_output_valid());

    assert_eq!(fired_events(&mut ctrl, 11_000), 0);
    let warped = ctrl
        .current_output_time()
        .expect("a valid output")
        .warped_time_in_scene_sec;
    assert!((warped - 10.5).abs() < 1e-3);
}

#[test]
fn updates_beyond_i32_max_do_not_spoil_the_cache() {
    let show = oracle::read_bytes("format/basic_show_with_yaw.skyb");
    let file = BinaryFile::new(&show).unwrap();
    let mut ctrl = ShowController::new(Screenplay::from_binary_file(&file).unwrap());
    ctrl.update_time_msec(5000);
    assert!(ctrl.current_output().position().is_some());

    let beyond = u32::try_from(i32::MAX).unwrap() + 1;
    ctrl.update_time_msec(beyond);
    assert!(ctrl.has_reached_end());
    assert_eq!(
        ctrl.current_output_time()
            .expect("a valid output")
            .time_msec,
        beyond
    );

    // Back to the earlier time, the output is computed again
    ctrl.update_time_msec(5000);
    assert!(!ctrl.has_reached_end());
    assert!(ctrl.current_output().position().is_some());
}
