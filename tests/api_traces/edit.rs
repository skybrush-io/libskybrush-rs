//! Replays the `cuts`, `landings` and `cleared_view` sections of an API trace:
//! modifications of the trajectory of the show file.
//!
//! The C library modifies a copy of the trajectory that it owns. The
//! replays modify the trajectory borrowed from the show file, which takes
//! the paths of the crate that avoid copying where they can;
//! `tests/trajectory_edits.rs` checks that owned trajectories give the same
//! bytes.

use serde_json::{Map, Value, json};
use skybrush::trajectory::{Trajectory, TrajectoryStatsCalculator};

use crate::common::Replay;
use crate::common::errors::result_name;
use crate::common::values::{self, f32_of, trajectory_samples, vector3, vector3_of};

/// How far the samples reach before and after a cut, in milliseconds.
const CUT_SAMPLING_WINDOW_MSEC: u32 = 1000;

/// How far the samples reach before the original landing time, in
/// milliseconds.
const LANDING_SAMPLING_LEAD_MSEC: u32 = 2000;

/// How far the samples reach after the end of the trajectory, in
/// milliseconds.
const MARGIN_MSEC: u32 = 1000;

pub fn replay_cuts(trajectory: Option<&Trajectory<'_>>, golden: &Value) -> Replay {
    let Some(trajectory) = trajectory else {
        return Replay::Absent;
    };
    let cases = golden.as_array().expect("cuts");
    Replay::Present(cases.iter().map(|case| cut(trajectory, case)).collect())
}

fn cut(original: &Trajectory<'_>, case: &Value) -> Value {
    let time_sec = f32_of(&case["time_sec"]);
    let mut trajectory = original.clone();
    let result = trajectory.cut_at(time_sec);

    let mut actual = Map::new();
    actual.insert("time_sec".to_owned(), case["time_sec"].clone());
    actual.insert("result".to_owned(), json!(result_name(&result)));
    if result.is_ok() {
        actual.insert(
            "duration_ms".to_owned(),
            json!(trajectory.total_duration_msec()),
        );
        actual.insert(
            "end_position_mm".to_owned(),
            vector3(trajectory.end_position()),
        );
        let cut_msec = values::float_to_msec(time_sec * 1000.0);
        actual.extend(trajectory_samples(
            &trajectory,
            cut_msec.saturating_sub(CUT_SAMPLING_WINDOW_MSEC),
            cut_msec + CUT_SAMPLING_WINDOW_MSEC,
        ));
    }
    Value::Object(actual)
}

pub fn replay_landings(trajectory: Option<&Trajectory<'_>>, golden: &Value) -> Replay {
    let Some(trajectory) = trajectory else {
        return Replay::Absent;
    };
    let cases = golden.as_array().expect("landings");
    Replay::Present(cases.iter().map(|case| landing(trajectory, case)).collect())
}

fn landing(original: &Trajectory<'_>, case: &Value) -> Value {
    let mut trajectory = original.clone();
    let mut stats = TrajectoryStatsCalculator::new(1000.0)
        .run(&trajectory)
        .expect("the default settings are valid");
    let original_landing_msec =
        values::float_to_msec(stats.landing.expect("landing time").landing_time_sec * 1000.0);

    let target = vector3_of(&case["target_mm"]);
    let landing_velocity = f32_of(&case["landing_velocity_mm_s"]);
    // null stands for NaN here: the variant without a terminal velocity
    let terminal_velocity = &case["terminal_velocity_mm_s"];
    let result = if terminal_velocity.is_null() {
        trajectory.replace_end_to_land_at(&mut stats, target, landing_velocity)
    } else {
        trajectory.replace_end_to_land_at_with_terminal_velocity(
            &mut stats,
            target,
            landing_velocity,
            f32_of(terminal_velocity),
        )
    };

    let mut actual: Map<String, Value> = [
        "case",
        "target_mm",
        "landing_velocity_mm_s",
        "terminal_velocity_mm_s",
    ]
    .into_iter()
    .map(|key| (key.to_owned(), case[key].clone()))
    .collect();
    actual.insert("result".to_owned(), json!(result_name(&result)));
    // Recorded on failure as well; the trajectory must be left intact then
    let duration_msec = trajectory.total_duration_msec();
    actual.insert("duration_ms".to_owned(), json!(duration_msec));
    actual.insert("stats".to_owned(), values::stats(&stats));
    if result.is_ok() {
        actual.extend(trajectory_samples(
            &trajectory,
            original_landing_msec.saturating_sub(LANDING_SAMPLING_LEAD_MSEC),
            duration_msec + MARGIN_MSEC,
        ));
    }
    Value::Object(actual)
}

pub fn replay_cleared_view(trajectory: Option<Trajectory<'_>>) -> Replay {
    let Some(mut trajectory) = trajectory else {
        return Replay::Absent;
    };
    trajectory.clear();
    Replay::Present(json!({
        "is_empty": trajectory.is_empty(),
        "duration_ms": trajectory.total_duration_msec(),
    }))
}
