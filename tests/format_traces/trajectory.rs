//! Replays the `trajectory` section of a format trace: the trajectory sampled
//! at the sample times of the trace.

use serde_json::{Map, Value, json};
use skybrush::basic_types::Vector3WithYaw;
use skybrush::trajectory::{Trajectory, TrajectoryPlayer};

use crate::common::Replay;
use crate::common::values::{sample_times, vector3};

pub fn replay(trajectory: Option<&Trajectory<'_>>, golden: &Map<String, Value>) -> Replay {
    let Some(trajectory) = trajectory else {
        return Replay::Absent;
    };
    let mut player = TrajectoryPlayer::new(trajectory);
    let mut sample = |query: &mut dyn FnMut(&mut TrajectoryPlayer<_>, f32) -> Vector3WithYaw| {
        sample_times(golden)
            .map(|(_, t)| vector3(query(&mut player, t)))
            .collect::<Value>()
    };
    let position = sample(&mut |player, t| player.position_at(t));
    let velocity = sample(&mut |player, t| player.velocity_at(t));
    let acceleration = sample(&mut |player, t| player.acceleration_at(t));
    Replay::Present(json!({
        "duration_ms": trajectory.total_duration_msec(),
        "position_mm": position,
        "velocity_mm_s": velocity,
        "acceleration_mm_s2": acceleration,
    }))
}
