//! Replays the `stats`, `proposed_takeoff_times` and `proposed_landing_times`
//! sections of an API trace.

use serde_json::{Map, Value, json};
use skybrush::trajectory::{Trajectory, TrajectoryStatComponents, TrajectoryStatsCalculator};

use crate::common::Replay;
use crate::common::errors::result_name;
use crate::common::values::{self, f32_of};

/// The statistics with each setting of the calculator in the golden section.
pub fn replay_stats(trajectory: Option<&Trajectory<'_>>, golden: &Value) -> Replay {
    let Some(trajectory) = trajectory else {
        return Replay::Absent;
    };
    let cases = golden.as_array().expect("settings of the calculator");
    Replay::Present(cases.iter().map(|case| stats(trajectory, case)).collect())
}

fn stats(trajectory: &Trajectory<'_>, case: &Value) -> Value {
    let components = u8::try_from(case["components"].as_u64().expect("components"))
        .expect("components fit in a byte");
    let calc = TrajectoryStatsCalculator {
        components: TrajectoryStatComponents::from_bits_truncate(components),
        takeoff_speed: f32_of(&case["takeoff_speed_mm_s"]),
        acceleration: f32_of(&case["acceleration_mm_s2"]),
        min_ascent: f32_of(&case["min_ascent_mm"]),
        preferred_descent: f32_of(&case["preferred_descent_mm"]),
        verticality_threshold: f32_of(&case["verticality_threshold_mm"]),
    };
    let result = calc.run(trajectory);

    // The settings are inputs, copied from the golden case
    let mut actual: Map<String, Value> = [
        "settings",
        "components",
        "takeoff_speed_mm_s",
        "acceleration_mm_s2",
        "min_ascent_mm",
        "preferred_descent_mm",
        "verticality_threshold_mm",
    ]
    .into_iter()
    .map(|key| (key.to_owned(), case[key].clone()))
    .collect();
    actual.insert("result".to_owned(), json!(result_name(&result)));
    if let Ok(stats) = result {
        actual.insert("stats".to_owned(), values::stats(&stats));
    }
    Value::Object(actual)
}

pub fn replay_proposed_takeoff_times(
    trajectory: Option<&Trajectory<'_>>,
    golden: &Value,
) -> Replay {
    let Some(trajectory) = trajectory else {
        return Replay::Absent;
    };
    let cases = golden.as_array().expect("cases");
    Replay::Present(
        cases
            .iter()
            .map(|case| {
                let (min_ascent, speed, acceleration) = (
                    &case["min_ascent_mm"],
                    &case["speed_mm_s"],
                    &case["acceleration_mm_s2"],
                );
                let time = trajectory.propose_takeoff_time_sec(
                    f32_of(min_ascent),
                    f32_of(speed),
                    f32_of(acceleration),
                );
                json!({
                    "min_ascent_mm": min_ascent,
                    "speed_mm_s": speed,
                    "acceleration_mm_s2": acceleration,
                    "takeoff_time_sec": time,
                })
            })
            .collect(),
    )
}

pub fn replay_proposed_landing_times(
    trajectory: Option<&Trajectory<'_>>,
    golden: &Value,
) -> Replay {
    let Some(trajectory) = trajectory else {
        return Replay::Absent;
    };
    let cases = golden.as_array().expect("cases");
    Replay::Present(
        cases
            .iter()
            .map(|case| {
                let (preferred_descent, verticality_threshold) = (
                    &case["preferred_descent_mm"],
                    &case["verticality_threshold_mm"],
                );
                let time = trajectory.propose_landing_time_sec(
                    f32_of(preferred_descent),
                    f32_of(verticality_threshold),
                );
                json!({
                    "preferred_descent_mm": preferred_descent,
                    "verticality_threshold_mm": verticality_threshold,
                    "landing_time_sec": time,
                })
            })
            .collect(),
    )
}
