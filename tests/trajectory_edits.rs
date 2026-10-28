//! Checks that modifying a trajectory borrowed from a show file gives the
//! same result as modifying an owned copy, and that the show file is not
//! modified.
//!
//! The API traces replay the modifications on borrowed trajectories, which
//! take the paths that avoid copying; the C library modifies an owned copy.
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)]

mod common;

use common::oracle;
use common::values::{f32_of, vector3_of};
use skybrush::formats::binary::BinaryFile;
use skybrush::trajectory::{Trajectory, TrajectoryStatsCalculator};

/// The show files of the traces with their API traces.
fn shows() -> impl Iterator<Item = (String, Vec<u8>, serde_json::Value)> {
    oracle::trace_names("api", &["tolerances", "scenarios"])
        .into_iter()
        .map(|name| {
            let show = oracle::read_bytes(&format!("format/{name}.skyb"));
            let golden = oracle::read_json(&format!("api/{name}.json"));
            (name, show, golden)
        })
}

#[test]
fn borrowed_and_owned_trajectories_are_cut_alike() {
    for (name, show, golden) in shows() {
        let file = BinaryFile::new(&show).unwrap();
        let Some(borrowed) = Trajectory::from_binary_file(&file).unwrap() else {
            continue;
        };
        for case in golden["cuts"].as_array().unwrap() {
            let time_sec = f32_of(&case["time_sec"]);
            let mut owned = borrowed.clone().into_owned();
            let mut view = borrowed.clone();
            owned.cut_at(time_sec).unwrap();
            view.cut_at(time_sec).unwrap();
            assert_eq!(owned.bytes(), view.bytes(), "{name}: cut at {time_sec}");
        }
        assert_eq!(show, oracle::read_bytes(&format!("format/{name}.skyb")));
    }
}

#[test]
fn borrowed_and_owned_trajectories_land_alike() {
    for (name, show, golden) in shows() {
        let file = BinaryFile::new(&show).unwrap();
        let Some(borrowed) = Trajectory::from_binary_file(&file).unwrap() else {
            continue;
        };
        let stats = TrajectoryStatsCalculator::new(1000.0)
            .run(&borrowed)
            .unwrap();
        for case in golden["landings"].as_array().unwrap() {
            let target = vector3_of(&case["target_mm"]);
            let velocity = f32_of(&case["landing_velocity_mm_s"]);
            let (mut owned, mut view) = (borrowed.clone().into_owned(), borrowed.clone());
            let (mut owned_stats, mut view_stats) = (stats, stats);
            let owned_result = owned.replace_end_to_land_at(&mut owned_stats, target, velocity);
            let view_result = view.replace_end_to_land_at(&mut view_stats, target, velocity);
            let label = format!("{name}: {}", case["case"]);
            assert_eq!(owned_result, view_result, "{label}");
            assert_eq!(owned.bytes(), view.bytes(), "{label}");
            assert_eq!(owned_stats, view_stats, "{label}");
        }
    }
}

#[test]
fn cuts_at_segment_boundaries_keep_borrowing() {
    for (name, show, _) in shows() {
        let file = BinaryFile::new(&show).unwrap();
        let Some(mut trajectory) = Trajectory::from_binary_file(&file).unwrap() else {
            continue;
        };
        let (state, _) = trajectory.segment_at(0.0);
        let end_sec = state.segment().end_time_sec();
        trajectory.cut_at(end_sec).unwrap();
        let range = show.as_ptr_range();
        assert!(
            range.contains(&trajectory.bytes().as_ptr()),
            "{name}: the cut trajectory does not borrow from the show file"
        );
    }
}
