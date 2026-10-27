//! Replays the `trajectory` section of an API trace: the duration, end
//! position and bounding box of the trajectory.

use serde_json::json;
use skybrush::trajectory::Trajectory;

use crate::common::Replay;
use crate::common::values::{interval, vector3};

pub fn replay(trajectory: Option<&Trajectory<'_>>) -> Replay {
    let Some(trajectory) = trajectory else {
        return Replay::Absent;
    };
    let bounding_box = trajectory.axis_aligned_bounding_box();
    Replay::Present(json!({
        "duration_ms": trajectory.total_duration_msec(),
        "end_position_mm": vector3(trajectory.end_position()),
        "bounding_box_mm": [
            interval(bounding_box.x),
            interval(bounding_box.y),
            interval(bounding_box.z),
        ],
    }))
}
