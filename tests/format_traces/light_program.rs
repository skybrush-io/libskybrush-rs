//! Replays the `light_program` section of a format trace: the time when the
//! light program ends, and its colors at the sample times of the trace.

use serde_json::{Map, Value, json};
use skybrush::lights::{LightPlayer, LightProgram};

use crate::common::Replay;
use crate::common::lights::{color, end_msec};
use crate::common::values::sample_times;

/// How far the generator of the traces scans a light program for its end, in
/// milliseconds.
const MAX_LIGHT_PROGRAM_DURATION_MSEC: u64 = 3_600_000;

pub fn replay(program: Option<&LightProgram<'_>>, golden: &Map<String, Value>) -> Replay {
    let Some(program) = program else {
        return Replay::Absent;
    };
    let mut player = LightPlayer::new(program);
    let colors: Value = sample_times(golden)
        .map(|(msec, _)| color(player.color_at(u64::from(msec))))
        .collect();
    Replay::Present(json!({
        "ends_at_ms": end_msec(program, MAX_LIGHT_PROGRAM_DURATION_MSEC),
        "color": colors,
    }))
}
