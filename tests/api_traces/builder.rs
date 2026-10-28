//! Replays the `builder` section of `scenarios.json`: sequences of operations
//! of the trajectory builder, and the segments of the resulting trajectories.

use serde_json::{Map, Value, json};
use skybrush::trajectory::{Trajectory, TrajectoryBuilder, TrajectoryPlayer};

use crate::common::Replay;
use crate::common::errors::result_name;
use crate::common::values::{f32_of, u32_of, vector3, vector3_with_yaw_of};

/// The relative times within a segment where it is sampled.
const SEGMENT_SAMPLES: [f32; 5] = [0.0, 0.25, 0.5, 0.75, 1.0];

pub fn replay(golden: &Value) -> Replay {
    let scenarios = golden.as_array().expect("builder scenarios");
    Replay::Present(scenarios.iter().map(scenario).collect())
}

fn scenario(golden: &Value) -> Value {
    let scale = u8::try_from(u32_of(&golden["scale"])).expect("scale fits in a byte");
    let mut actual = Map::new();
    actual.insert("scenario".to_owned(), golden["scenario"].clone());
    actual.insert("scale".to_owned(), json!(scale));
    let builder = TrajectoryBuilder::new(scale);
    actual.insert("init_result".to_owned(), json!(result_name(&builder)));
    if let Ok(mut builder) = builder {
        actual.insert("ops".to_owned(), replay_ops(&mut builder, &golden["ops"]));
        let trajectory = builder.build().expect("a valid trajectory");
        actual.insert("trajectory".to_owned(), summary(&trajectory));
    }
    Value::Object(actual)
}

/// Applies the operations of a scenario to a builder; returns them with
/// their results.
pub fn replay_ops(builder: &mut TrajectoryBuilder, ops: &Value) -> Value {
    let ops = ops.as_array().expect("operations");
    ops.iter().map(|op| replay_op(builder, op)).collect()
}

fn replay_op(builder: &mut TrajectoryBuilder, op: &Value) -> Value {
    let name = op["op"].as_str().expect("name of the operation");
    let (inputs, result): (&[&str], _) = match name {
        "set_start_position" => (
            &["position_mm"],
            builder.set_start_position(vector3_with_yaw_of(&op["position_mm"])),
        ),
        "append_line" => (
            &["target_mm", "duration_ms"],
            builder.append_line(
                vector3_with_yaw_of(&op["target_mm"]),
                u32_of(&op["duration_ms"]),
            ),
        ),
        "append_cubic_bezier" => (
            &["control1_mm", "control2_mm", "target_mm", "duration_ms"],
            builder.append_cubic_bezier(
                vector3_with_yaw_of(&op["control1_mm"]),
                vector3_with_yaw_of(&op["control2_mm"]),
                vector3_with_yaw_of(&op["target_mm"]),
                u32_of(&op["duration_ms"]),
            ),
        ),
        "hold_position_for" => (
            &["duration_ms"],
            builder.hold_position_for(u32_of(&op["duration_ms"])),
        ),
        "move_to_in_time" => (
            &["target_mm", "duration_ms", "max_acceleration_mm_s2"],
            builder.move_to_in_time(
                vector3_with_yaw_of(&op["target_mm"]),
                u32_of(&op["duration_ms"]),
                f32_of(&op["max_acceleration_mm_s2"]),
            ),
        ),
        _ => panic!("unknown builder operation: {name}"),
    };
    let mut actual = Map::new();
    actual.insert("op".to_owned(), json!(name));
    for &key in inputs {
        actual.insert(key.to_owned(), op[key].clone());
    }
    actual.insert("result".to_owned(), json!(result_name(&result)));
    Value::Object(actual)
}

/// The start position and the segments of a built trajectory, sampled at a
/// few relative times. Positions are in the units of the scale, since the
/// builder quantizes them to those units.
fn summary(trajectory: &Trajectory<'_>) -> Value {
    let scale = trajectory.scale();
    let mut player = TrajectoryPlayer::new(trajectory);
    // Sampling moves a player, so a second one samples the segments
    let mut sampler = TrajectoryPlayer::new(trajectory);
    let mut segments = Vec::new();
    while player.has_more_segments() {
        let segment = *player.current_segment();
        let positions: Vec<Value> = SEGMENT_SAMPLES
            .iter()
            .map(|&ratio| {
                let t = segment.start_time_sec() + ratio * segment.duration_sec();
                let position = sampler.position_at(t);
                json!([position.x / scale, position.y / scale, position.z / scale])
            })
            .collect();
        segments.push(json!({
            "start_ms": segment.start_time_msec(),
            "duration_ms": segment.duration_msec(),
            "position_units": positions,
        }));
        player.build_next_segment();
    }
    #[expect(
        clippy::cast_possible_truncation,
        clippy::cast_sign_loss,
        reason = "the scale of a built trajectory is an integer from 1 to 127"
    )]
    let integer_scale = scale as u8;
    json!({
        "is_empty": trajectory.is_empty(),
        "start_position_mm": vector3(trajectory.start_position()),
        "scale": integer_scale,
        "duration_ms": trajectory.total_duration_msec(),
        "segments": segments,
    })
}
