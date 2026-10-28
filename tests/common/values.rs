//! Conversions between the values of the crate and the values of the oracle.

use serde_json::{Map, Value, json};
use skybrush::basic_types::{Interval, Vector3, Vector3WithYaw};
use skybrush::trajectory::{Trajectory, TrajectoryPlayer, TrajectoryStats};

/// A position or another vector, as `[x, y, z]`. The yaw is not recorded.
/// Non-finite components become `null`.
pub fn vector3(vector: Vector3WithYaw) -> Value {
    json!([vector.x, vector.y, vector.z])
}

/// A position or another vector with its yaw, as `[x, y, z, yaw]`.
pub fn vector3_with_yaw(vector: Vector3WithYaw) -> Value {
    json!([vector.x, vector.y, vector.z, vector.yaw])
}

/// A vector recorded as `[x, y, z, yaw]`.
pub fn vector3_with_yaw_of(value: &Value) -> Vector3WithYaw {
    let component = |index: usize| f32_of(&value[index]);
    Vector3WithYaw {
        x: component(0),
        y: component(1),
        z: component(2),
        yaw: component(3),
    }
}

/// A vector recorded as `[x, y, z]`.
pub fn vector3_of(value: &Value) -> Vector3 {
    Vector3 {
        x: f32_of(&value[0]),
        y: f32_of(&value[1]),
        z: f32_of(&value[2]),
    }
}

/// The positions and velocities along a trajectory every 100 ms, from
/// `from_msec` rounded down to a multiple of 100 ms, up to `to_msec`, as the
/// generators of the C library record them.
pub fn trajectory_samples(
    trajectory: &Trajectory<'_>,
    from_msec: u32,
    to_msec: u32,
) -> Map<String, Value> {
    const INTERVAL_MSEC: u32 = 100;
    let from_msec = from_msec - from_msec % INTERVAL_MSEC;
    let times: Vec<f32> = (from_msec..=to_msec)
        .step_by(INTERVAL_MSEC as usize)
        .map(msec_to_sec)
        .collect();
    let mut player = TrajectoryPlayer::new(trajectory);
    let positions: Value = times
        .iter()
        .map(|&t| vector3(player.position_at(t)))
        .collect();
    let velocities: Value = times
        .iter()
        .map(|&t| vector3(player.velocity_at(t)))
        .collect();
    let mut result = Map::new();
    result.insert("samples_from_ms".to_owned(), json!(from_msec));
    result.insert("position_mm".to_owned(), positions);
    result.insert("velocity_mm_s".to_owned(), velocities);
    result
}

/// An interval, as `[min, max]`.
pub fn interval(interval: Interval) -> Value {
    json!([interval.min, interval.max])
}

/// The statistics of a trajectory, with the components that were calculated.
/// The initial position is recorded even if it was not calculated: the C
/// library leaves it zero then.
pub fn stats(stats: &TrajectoryStats) -> Value {
    let mut result = Map::new();
    let mut insert = |key: &str, value: Value| result.insert(key.to_owned(), value);
    insert(
        "initial_position_mm",
        vector3(stats.initial_pos.unwrap_or_default()),
    );
    if let Some(duration_msec) = stats.duration_msec {
        insert("duration_ms", json!(duration_msec));
    }
    if let Some(takeoff) = stats.takeoff {
        insert("earliest_above_sec", json!(takeoff.earliest_above_sec));
        insert("takeoff_time_sec", json!(takeoff.takeoff_time_sec));
    }
    if let Some(distance) = stats.start_to_end_distance_xy {
        insert("start_to_end_distance_xy_mm", json!(distance));
    }
    if let Some(landing) = stats.landing {
        insert("landing_time_sec", json!(landing.landing_time_sec));
        insert(
            "position_at_landing_mm",
            vector3(landing.pos_at_landing_time),
        );
        insert(
            "velocity_at_landing_mm_s",
            vector3(landing.vel_at_landing_time),
        );
    }
    Value::Object(result)
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

/// A float input of a computation. Inputs that are not finite are recorded
/// as `null`; all of them are positive infinity in the traces.
pub fn f32_of(value: &Value) -> f32 {
    if value.is_null() {
        return f32::INFINITY;
    }
    to_f32(
        value
            .as_f64()
            .unwrap_or_else(|| panic!("not a number: {value}")),
    )
}

#[expect(
    clippy::cast_possible_truncation,
    reason = "the traces hold f32 values"
)]
pub fn to_f32(value: f64) -> f32 {
    value as f32
}

/// Milliseconds from a float, truncated like a cast to `uint32_t` in C.
#[expect(
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss,
    reason = "the conversion of the C library"
)]
pub fn float_to_msec(msec: f32) -> u32 {
    msec as u32
}

pub fn u32_of(value: &Value) -> u32 {
    let value = value
        .as_u64()
        .unwrap_or_else(|| panic!("not an integer: {value}"));
    u32::try_from(value).unwrap_or_else(|_| panic!("out of range: {value}"))
}
