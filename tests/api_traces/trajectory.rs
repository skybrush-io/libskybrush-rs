//! Replays the `trajectory` section of an API trace (the duration, end
//! position and bounding box of the trajectory), and the `bounding_boxes`
//! section of `scenarios.json`.

use serde_json::{Value, json};
use skybrush::basic_types::BoundingBox;
use skybrush::trajectory::Trajectory;

use crate::common::Replay;
use crate::common::oracle::decode_hex;
use crate::common::values::{interval, vector3};

pub fn replay(trajectory: Option<&Trajectory<'_>>) -> Replay {
    let Some(trajectory) = trajectory else {
        return Replay::Absent;
    };
    let bounding_box = trajectory.axis_aligned_bounding_box();
    Replay::Present(json!({
        "duration_ms": trajectory.total_duration_msec(),
        "end_position_mm": vector3(trajectory.end_position()),
        "bounding_box_mm": bounding_box_to_json(bounding_box),
    }))
}

/// The bounding boxes of the trajectories in the `bounding_boxes` section of
/// `scenarios.json`, given as the hex-encoded bodies of their blocks.
pub fn replay_bounding_boxes(golden: &Value) -> Replay {
    let cases = golden.as_array().expect("bounding box scenarios");
    Replay::Present(
        cases
            .iter()
            .map(|case| {
                let body = decode_hex(case["trajectory"].as_str().expect("hex body"));
                let trajectory = Trajectory::from_buffer(&body).expect("a valid trajectory");
                json!({
                    "scenario": case["scenario"],
                    "trajectory": case["trajectory"],
                    "result": "SUCCESS",
                    "bounding_box_mm": bounding_box_to_json(trajectory.axis_aligned_bounding_box()),
                })
            })
            .collect(),
    )
}

fn bounding_box_to_json(bounding_box: BoundingBox) -> Value {
    json!([
        interval(bounding_box.x),
        interval(bounding_box.y),
        interval(bounding_box.z),
    ])
}
