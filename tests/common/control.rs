//! Control outputs as the oracle records them.

use serde_json::{Map, Value, json};
use skybrush::basic_types::Vector3;
use skybrush::control::ControlOutput;

use super::lights::color;

/// The components of an output: position, velocity, yaw, yaw rate and color,
/// each `null` if it is not set.
pub fn output(output: &ControlOutput) -> Map<String, Value> {
    let vector = |vector: Option<Vector3>| vector.map_or(Value::Null, |v| json!([v.x, v.y, v.z]));
    let mut result = Map::new();
    result.insert("position_mm".into(), vector(output.position()));
    result.insert("velocity_mm_s".into(), vector(output.velocity()));
    result.insert("yaw_deg".into(), json!(output.yaw()));
    result.insert("yaw_rate_deg_s".into(), json!(output.yaw_rate()));
    result.insert("color".into(), output.color().map_or(Value::Null, color));
    result
}
