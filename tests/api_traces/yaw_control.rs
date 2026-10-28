//! Replays the `constant_yaw` section of `scenarios.json`: yaw control objects
//! that keep a constant yaw.

use serde_json::{Value, json};
use skybrush::yaw_control::{YawControl, YawPlayer};

use crate::common::Replay;
use crate::common::errors::result_name;
use crate::common::values::f32_of;

pub fn replay_constant_yaw(golden: &Value) -> Replay {
    let cases = golden.as_array().expect("cases");
    Replay::Present(
        cases
            .iter()
            .map(|case| {
                let yaw_deg = f32_of(&case["yaw_deg"]);
                let mut ctrl = YawControl::new();
                let result = ctrl.set_constant_yaw(yaw_deg);
                let mut player = YawPlayer::new(&ctrl);
                json!({
                    "yaw_deg": yaw_deg,
                    "result": result_name(&result),
                    "is_empty": ctrl.is_empty(),
                    "yaw_at_start_deg": player.yaw_at(0.0),
                    "yaw_later_deg": player.yaw_at(100.0),
                    "yaw_rate_deg_s": player.yaw_rate_at(10.0),
                })
            })
            .collect(),
    )
}
