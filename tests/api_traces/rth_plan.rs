//! Replays the `rth_plan_queries` and `rth_trajectories` sections of an API
//! trace, and the `rth_conversions` and `empty_rth_plan` sections of
//! `scenarios.json`: queries of RTH plans, and the trajectories of their
//! entries.

use serde_json::{Map, Value, json};
use skybrush::basic_types::{Vector2, Vector3};
use skybrush::rth_plan::{RthPlan, RthPlanEntry, RthPlanEntryFlags};
use skybrush::trajectory::{Trajectory, TrajectoryPlayer};

use crate::common::Replay;
use crate::common::errors::{ErrorName, result_name};
use crate::common::values::{
    f32_of, float_to_msec, msec_to_sec, trajectory_samples, u32_of, vector3_of,
};

/// The times of the queries of the plans, in seconds. The oracle records the
/// non-finite ones as `null`, so they are listed here; the replay records
/// them, so the comparison checks them.
const QUERY_TIMES_SEC: [f32; 7] = [
    -10.0,
    -0.001,
    0.0,
    f32::NAN,
    f32::INFINITY,
    f32::NEG_INFINITY,
    1e9,
];

/// How long the generator samples a trajectory after its end, and the
/// interval of the samples, in milliseconds.
const MARGIN_MSEC: u32 = 1000;
const SAMPLE_INTERVAL_MSEC: u32 = 100;

pub fn replay_queries(plan: Option<RthPlan<'_>>) -> Replay {
    let Some(mut plan) = plan else {
        return Replay::Absent;
    };
    let points: Vec<Value> = (0..=plan.num_points())
        .map(|index| match plan.point(index) {
            Some(point) => {
                json!({ "index": index, "result": "SUCCESS", "point_mm": [point.x, point.y] })
            }
            None => json!({ "index": index, "result": "EINVAL" }),
        })
        .collect();
    let evaluate_at: Vec<Value> = QUERY_TIMES_SEC
        .iter()
        .map(|&time_sec| {
            let mut result = Map::new();
            result.insert("query_time_sec".into(), json!(time_sec));
            result.extend(evaluation(&plan, time_sec));
            Value::Object(result)
        })
        .collect();
    let mut queries = json!({
        "num_entries": plan.num_entries(),
        "num_points": plan.num_points(),
        "is_empty": plan.is_empty(),
        "default_acceleration_limit_mm_s2": plan.default_acceleration_limit(),
        "default_landing_altitude_mm": plan.default_landing_altitude(),
        "default_landing_velocity_mm_s": plan.default_landing_velocity(),
        "has_default_landing_velocity": plan.has_default_landing_velocity(),
        "points": points,
        "evaluate_at": evaluate_at,
    });

    // Changing the defaults affects the entries that do not override them
    plan.set_default_landing_altitude(1500.0);
    plan.clear_default_landing_velocity();
    let mut after = Map::new();
    after.insert(
        "default_landing_altitude_mm".into(),
        json!(plan.default_landing_altitude()),
    );
    after.insert(
        "has_default_landing_velocity".into(),
        json!(plan.has_default_landing_velocity()),
    );
    let mut at_20_sec = evaluation(&plan, 20.0);
    if let Some(result) = at_20_sec.remove("result") {
        after.insert("result_at_20_sec".into(), result);
    }
    after.extend(at_20_sec);
    queries["after_changing_defaults"] = Value::Object(after);
    Replay::Present(queries)
}

/// The trajectory of each distinct entry of the plan along the trajectory of
/// the show, starting from the position of the drone at the time of the
/// entry.
pub fn replay_trajectories(
    trajectory: Option<&Trajectory<'_>>,
    plan: Option<&RthPlan<'_>>,
) -> Replay {
    let (Some(trajectory), Some(plan)) = (trajectory, plan) else {
        return Replay::Absent;
    };
    let mut player = TrajectoryPlayer::new(trajectory);
    let mut last_entry_time = f32::NAN;
    let mut result = Vec::new();
    for t in (0..=trajectory.total_duration_msec()).step_by(SAMPLE_INTERVAL_MSEC as usize) {
        let entry = plan.evaluate_at(msec_to_sec(t)).expect("valid entries");
        // The times of the entries are integers or the exact query times
        #[expect(clippy::float_cmp, reason = "as in the generator")]
        if entry.time_sec == last_entry_time {
            continue;
        }
        last_entry_time = entry.time_sec;
        let start = player.position_at(entry.time_sec).xyz();
        let mut item = Map::new();
        item.insert("entry_time_sec".into(), json!(entry.time_sec));
        item.insert("start_mm".into(), json!([start.x, start.y, start.z]));
        item.extend(conversion(
            &entry,
            start,
            float_to_msec(entry.time_sec * 1000.0),
        ));
        result.push(Value::Object(item));
    }
    Replay::Present(Value::Array(result))
}

pub fn replay_conversions(golden: &Value) -> Replay {
    let scenarios = golden.as_array().expect("scenarios");
    Replay::Present(
        scenarios
            .iter()
            .map(|scenario| {
                let start = vector3_of(&scenario["start_mm"]);
                let entry = entry_of(&scenario["entry"]);
                let samples_from_msec = if entry.time_sec > 0.0 {
                    float_to_msec(entry.time_sec * 1000.0)
                } else {
                    0
                };
                let mut result = Map::new();
                result.insert("scenario".into(), scenario["scenario"].clone());
                result.insert("start_mm".into(), json!([start.x, start.y, start.z]));
                result.insert("entry".into(), entry_json(&entry));
                result.extend(conversion(&entry, start, samples_from_msec));
                Value::Object(result)
            })
            .collect(),
    )
}

pub fn replay_empty_plan() -> Replay {
    let plan = RthPlan::new();
    Replay::Present(json!({
        "is_empty": plan.is_empty(),
        "num_entries": plan.num_entries(),
        "num_points": plan.num_points(),
        "evaluate_at_result": result_name(&plan.evaluate_at(5.0)),
        "default_landing_altitude_mm": plan.default_landing_altitude(),
    }))
}

/// The result of evaluating a plan, with a summary of the entry.
fn evaluation(plan: &RthPlan<'_>, time_sec: f32) -> Map<String, Value> {
    let mut result = Map::new();
    match plan.evaluate_at(time_sec) {
        Ok(entry) => {
            result.insert("result".into(), json!("SUCCESS"));
            result.insert(
                "entry".into(),
                json!({
                    "time_sec": entry.time_sec,
                    "flags": entry.flags.bits(),
                    "duration_sec": entry.duration_sec,
                    "landing_target_mm": [entry.landing_target.x, entry.landing_target.y],
                    "landing_altitude_mm": entry.landing_altitude,
                    "landing_velocity_mm_s": entry.landing_velocity,
                }),
            );
        }
        Err(err) => {
            result.insert("result".into(), json!(err.error_name()));
        }
    }
    result
}

/// The result of converting an entry to a trajectory, with its samples.
fn conversion(entry: &RthPlanEntry, start: Vector3, samples_from_msec: u32) -> Map<String, Value> {
    let mut result = Map::new();
    match Trajectory::from_rth_plan_entry(entry, start) {
        Ok(trajectory) => {
            let duration_msec = trajectory.total_duration_msec();
            result.insert("result".into(), json!("SUCCESS"));
            result.insert("duration_ms".into(), json!(duration_msec));
            result.extend(trajectory_samples(
                &trajectory,
                samples_from_msec,
                duration_msec + MARGIN_MSEC,
            ));
        }
        Err(err) => {
            result.insert("result".into(), json!(err.error_name()));
        }
    }
    result
}

fn entry_of(golden: &Value) -> RthPlanEntry {
    let flags = u8::try_from(u32_of(&golden["flags"])).expect("flags in a byte");
    RthPlanEntry {
        time_sec: f32_of(&golden["time_sec"]),
        flags: RthPlanEntryFlags::from_bits(flags),
        landing_target: Vector2 {
            x: f32_of(&golden["landing_target_mm"][0]),
            y: f32_of(&golden["landing_target_mm"][1]),
        },
        arrival_altitude: f32_of(&golden["arrival_altitude_mm"]),
        duration_sec: f32_of(&golden["duration_sec"]),
        pre_delay_sec: f32_of(&golden["pre_delay_sec"]),
        post_delay_sec: f32_of(&golden["post_delay_sec"]),
        pre_neck: f32_of(&golden["pre_neck_mm"]),
        pre_neck_duration_sec: f32_of(&golden["pre_neck_duration_sec"]),
        // `null` stands for infinity or NaN here, which mean the same: no
        // limit and no descent
        max_acceleration: f32_of(&golden["max_acceleration_mm_s2"]),
        landing_velocity: f32_of(&golden["landing_velocity_mm_s"]),
        landing_altitude: f32_of(&golden["landing_altitude_mm"]),
    }
}

fn entry_json(entry: &RthPlanEntry) -> Value {
    json!({
        "time_sec": entry.time_sec,
        "flags": entry.flags.bits(),
        "landing_target_mm": [entry.landing_target.x, entry.landing_target.y],
        "arrival_altitude_mm": entry.arrival_altitude,
        "duration_sec": entry.duration_sec,
        "pre_delay_sec": entry.pre_delay_sec,
        "post_delay_sec": entry.post_delay_sec,
        "pre_neck_mm": entry.pre_neck,
        "pre_neck_duration_sec": entry.pre_neck_duration_sec,
        "max_acceleration_mm_s2": entry.max_acceleration,
        "landing_velocity_mm_s": entry.landing_velocity,
        "landing_altitude_mm": entry.landing_altitude,
    })
}
