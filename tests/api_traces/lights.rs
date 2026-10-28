//! Replays the `light_player` section of an API trace and the
//! `light_programs` section of `scenarios.json`: the time instants where the
//! light player wakes up, and the colors of light programs.

use serde_json::{Map, Value, json};
use skybrush::colors::RgbColor;
use skybrush::lights::{LightPlayer, LightProgram};

use crate::common::Replay;
use crate::common::errors::result_name;
use crate::common::lights::color;
use crate::common::oracle::{decode_hex, encode_hex};

/// How far the generator of the API traces follows the light player, in
/// milliseconds, and how many changes it records at most.
const MAX_LIGHT_PROGRAM_DURATION_MSEC: u64 = 3_600_000;
const MAX_LIGHT_PROGRAM_CHANGES: usize = 100_000;

/// How many changes the generator of the scenarios records at most, and up
/// to when it samples the colors, in milliseconds.
const MAX_SCENARIO_CHANGES: usize = 600;
const SCENARIO_SAMPLES_UNTIL_MSEC: u64 = 12_000;
const SAMPLE_INTERVAL_MSEC: u64 = 100;

pub fn replay_light_player(program: Option<&LightProgram<'_>>) -> Replay {
    let Some(program) = program else {
        return Replay::Absent;
    };
    let mut player = LightPlayer::new(program);
    let mut changes = Vec::new();
    let mut ended = false;
    let mut t = 0;
    while t <= MAX_LIGHT_PROGRAM_DURATION_MSEC && changes.len() < MAX_LIGHT_PROGRAM_CHANGES {
        ended = player.seek(t);
        let next = player.next_timestamp();
        changes.push(json!([t, next]));
        if ended {
            break;
        }
        t = if next > t { next } else { t + 1 };
    }
    Replay::Present(json!({ "changes_ms": changes, "ended": ended }))
}

pub fn replay_scenarios(golden: &Value) -> Replay {
    let scenarios = golden.as_array().expect("light program scenarios");
    Replay::Present(scenarios.iter().map(scenario).collect())
}

fn scenario(golden: &Value) -> Value {
    let mut actual = Map::new();
    actual.insert("scenario".to_owned(), golden["scenario"].clone());
    let program = if let Some(constant) = golden.get("constant_color") {
        let mut program = LightProgram::new();
        let result = program.set_constant_color(rgb_of(constant));
        actual.insert("constant_color".to_owned(), constant.clone());
        actual.insert("result".to_owned(), json!(result_name(&result)));
        program
    } else {
        let hex = golden["bytecode_hex"].as_str().expect("bytecode");
        LightProgram::from_bytes(decode_hex(hex))
    };
    actual.extend(playback(&program));
    Value::Object(actual)
}

/// The bytecode, the time instants where the player wakes up, and the colors
/// sampled every 100 ms, as the generator of the scenarios records them.
fn playback(program: &LightProgram<'_>) -> Map<String, Value> {
    let mut player = LightPlayer::new(program);
    let mut changes = Vec::new();
    let mut ended = false;
    let mut t = 0;
    while changes.len() < MAX_SCENARIO_CHANGES {
        ended = player.seek(t);
        let next = player.next_timestamp();
        changes.push(json!([t, next]));
        if ended || next <= t {
            break;
        }
        t = next;
    }
    let colors: Value = (0..=SCENARIO_SAMPLES_UNTIL_MSEC)
        .step_by(usize::try_from(SAMPLE_INTERVAL_MSEC).expect("small"))
        .map(|t| color(player.color_at(t)))
        .collect();

    let mut result = Map::new();
    result.insert(
        "bytecode_hex".to_owned(),
        json!(encode_hex(program.bytes())),
    );
    result.insert("changes_ms".to_owned(), changes.into());
    result.insert("ended".to_owned(), json!(ended));
    result.insert("color".to_owned(), colors);
    result
}

fn rgb_of(value: &Value) -> RgbColor {
    let component = |index: usize| {
        u8::try_from(value[index].as_u64().expect("color component")).expect("a byte")
    };
    RgbColor::new(component(0), component(1), component(2))
}
