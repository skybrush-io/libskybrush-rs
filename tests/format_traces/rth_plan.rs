//! Replays the `rth_plan` section of a format trace: the entry of the RTH
//! plan at the sample times of the trace.

use serde_json::{Map, Value, json};
use skybrush::rth_plan::RthPlan;

use crate::common::Replay;
use crate::common::values::sample_times;

pub fn replay(plan: Option<&RthPlan<'_>>, golden: &Map<String, Value>) -> Replay {
    let Some(plan) = plan else {
        return Replay::Absent;
    };
    let entries: Vec<Value> = sample_times(golden)
        .map(|(_, sec)| {
            let entry = plan.evaluate_at(sec).expect("valid entries");
            json!([
                entry.time_sec,
                entry.flags.bits(),
                entry.landing_target.x,
                entry.landing_target.y,
                entry.arrival_altitude,
                entry.duration_sec,
                entry.pre_delay_sec,
                entry.post_delay_sec,
                entry.pre_neck,
                entry.pre_neck_duration_sec,
                entry.max_acceleration,
                entry.landing_velocity,
                entry.landing_altitude,
            ])
        })
        .collect();
    Replay::Present(json!({
        "entry_fields": [
            "time_sec", "flags", "landing_target_x_mm", "landing_target_y_mm",
            "arrival_altitude_mm", "duration_sec", "pre_delay_sec", "post_delay_sec",
            "pre_neck_mm", "pre_neck_duration_sec", "max_acceleration_mm_s2",
            "landing_velocity_mm_s", "landing_altitude_mm",
        ],
        "entries": entries,
    }))
}
