//! Conversions between the values of the crate and the values of the oracle.

use serde_json::{Map, Value, json};
use skybrush::basic_types::{Interval, Vector3WithYaw};

/// A position or another vector, as `[x, y, z]`. The yaw is not recorded.
/// Non-finite components become `null`.
pub fn vector3(vector: Vector3WithYaw) -> Value {
    json!([vector.x, vector.y, vector.z])
}

/// An interval, as `[min, max]`.
pub fn interval(interval: Interval) -> Value {
    json!([interval.min, interval.max])
}

/// The time instants of the samples of a trace, in milliseconds and in
/// seconds, from its `sample_interval_ms` and `num_samples`. The seconds are
/// computed like the generators of the C library do.
pub fn sample_times(golden: &Map<String, Value>) -> impl Iterator<Item = (u32, f32)> + use<> {
    let interval = u32_of(&golden["sample_interval_ms"]);
    let count = u32_of(&golden["num_samples"]);
    (0..count).map(move |index| {
        let msec = index * interval;
        (msec, msec_to_sec(msec))
    })
}

/// Converts milliseconds to seconds like the C library: `msec / 1000.0f`.
#[expect(
    clippy::cast_precision_loss,
    reason = "the conversion of the C library"
)]
pub fn msec_to_sec(msec: u32) -> f32 {
    msec as f32 / 1000.0
}

pub fn u32_of(value: &Value) -> u32 {
    let value = value
        .as_u64()
        .unwrap_or_else(|| panic!("not an integer: {value}"));
    u32::try_from(value).unwrap_or_else(|_| panic!("out of range: {value}"))
}
