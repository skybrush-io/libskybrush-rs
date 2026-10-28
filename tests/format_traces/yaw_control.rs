//! Replays the `yaw_control` section of a format trace: the total duration of
//! the deltas, and the yaw and the yaw rate at the sample times of the trace.

use serde_json::{Map, Value, json};
use skybrush::yaw_control::{YawControl, YawPlayer};

use crate::common::Replay;
use crate::common::values::sample_times;

pub fn replay(ctrl: Option<&YawControl<'_>>, golden: &Map<String, Value>) -> Replay {
    let Some(ctrl) = ctrl else {
        return Replay::Absent;
    };
    let mut player = YawPlayer::new(ctrl);
    let duration_msec = player.total_duration_msec();
    let yaw: Vec<f32> = sample_times(golden)
        .map(|(_, sec)| player.yaw_at(sec))
        .collect();
    let yaw_rate: Vec<f32> = sample_times(golden)
        .map(|(_, sec)| player.yaw_rate_at(sec))
        .collect();
    Replay::Present(json!({
        "duration_ms": duration_msec,
        "yaw_deg": yaw,
        "yaw_rate_deg_s": yaw_rate,
    }))
}
