//! Replays the `motion` section of `scenarios.json`: the public motion
//! helpers.

use serde_json::{Map, Value, json};
use skybrush::motion;

use crate::common::Replay;
use crate::common::values::{f32_of, vector3_with_yaw, vector3_with_yaw_of};

pub fn replay(golden: &Value) -> Replay {
    let golden = golden.as_object().expect("motion");
    let mut actual = Map::new();
    for (key, cases) in golden {
        let cases = cases.as_array().expect("cases of a motion helper");
        let replay_case: fn(&Value) -> Value = match key.as_str() {
            "travel_time_for_distance" => travel_time_for_distance,
            "travel_velocity_for_distance" => travel_velocity_for_distance,
            "cubic_bezier_from_velocity_constraints" => cubic_bezier_from_velocity_constraints,
            // Left out, so that the comparison reports it as missing
            _ => continue,
        };
        actual.insert(key.clone(), cases.iter().map(replay_case).collect());
    }
    Replay::Present(Value::Object(actual))
}

fn travel_time_for_distance(case: &Value) -> Value {
    let (distance, speed, acceleration) = (
        &case["distance_mm"],
        &case["speed_mm_s"],
        &case["acceleration_mm_s2"],
    );
    let time =
        motion::travel_time_for_distance(f32_of(distance), f32_of(speed), f32_of(acceleration));
    json!({
        "distance_mm": distance,
        "speed_mm_s": speed,
        "acceleration_mm_s2": acceleration,
        "travel_time_sec": time,
    })
}

fn travel_velocity_for_distance(case: &Value) -> Value {
    let (distance, time, acceleration) = (
        &case["distance_mm"],
        &case["time_sec"],
        &case["acceleration_mm_s2"],
    );
    let velocity =
        motion::travel_velocity_for_distance(f32_of(distance), f32_of(time), f32_of(acceleration));
    json!({
        "distance_mm": distance,
        "time_sec": time,
        "acceleration_mm_s2": acceleration,
        "velocity_mm_s": velocity,
    })
}

fn cubic_bezier_from_velocity_constraints(case: &Value) -> Value {
    let inputs = [
        "start_mm",
        "start_velocity_mm_s",
        "end_mm",
        "end_velocity_mm_s",
        "duration_sec",
    ];
    let mut actual: Map<String, Value> = inputs
        .into_iter()
        .map(|key| (key.to_owned(), case[key].clone()))
        .collect();
    let controls = motion::cubic_bezier_from_velocity_constraints(
        vector3_with_yaw_of(&case["start_mm"]),
        vector3_with_yaw_of(&case["start_velocity_mm_s"]),
        vector3_with_yaw_of(&case["end_mm"]),
        vector3_with_yaw_of(&case["end_velocity_mm_s"]),
        f32_of(&case["duration_sec"]),
    );
    if let Some((control1, control2)) = controls {
        actual.insert("result".to_owned(), json!("SUCCESS"));
        actual.insert("control1_mm".to_owned(), vector3_with_yaw(control1));
        actual.insert("control2_mm".to_owned(), vector3_with_yaw(control2));
    } else {
        actual.insert("result".to_owned(), json!("EINVAL"));
    }
    Value::Object(actual)
}
