//! Replays the conformance cases of the `.skyb` format
//! (`tests/oracle/conformance`).
//!
//! Each case yields one item `<id>/file` for opening the file (which includes
//! the check that the block types not listed in the case are missing), and
//! one item `<id>/<block type>` for each block type listed in the case.
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)]

mod common;

use common::Replay;
use common::compare::{Comparator, Tolerance, Tolerances};
use common::errors::{ErrorName, result_name};
use common::events;
use common::lights;
use common::oracle;
use common::ratchet::{Failure, Ratchet};
use common::values;
use serde_json::{Map, Value, json};
use skybrush::events::EventList;
use skybrush::formats::binary::{BinaryBlock, BinaryBlockType, BinaryFile};
use skybrush::gcs_light_control::GcsLightControlSetup;
use skybrush::lights::{LightPlayer, LightProgram};
use skybrush::metadata::ShowMetadata;
use skybrush::rth_plan::RthPlan;
use skybrush::trajectory::{Trajectory, TrajectoryPlayer};
use skybrush::yaw_control::{YawControl, YawPlayer};

/// Names of the columns of the tuples in the cases; the suffixes of the names
/// select the tolerances.
const COLUMNS: &[(&str, &[&str])] = &[
    ("positions", &["time_ms", "x_mm", "y_mm", "z_mm"]),
    ("yaw_deg", &["time_ms", "yaw_deg"]),
    ("colors", &["time_ms", "red", "green", "blue"]),
    ("events", &["time_ms", "type", "subtype", "payload"]),
];

#[test]
fn conformance_cases() {
    let cases = oracle::read_json("conformance/cases.json");
    oracle::assert_version("cases.json", &cases, 1);
    let tolerances = tolerances(&cases["tolerances"]);
    let block_types = block_types();

    let mut ratchet = Ratchet::new("conformance");
    for case in cases["cases"].as_array().expect("cases") {
        let id = case["id"].as_str().expect("id of the case");
        let bytes = oracle::decode_hex(case["bytes"].as_str().expect("bytes of the case"));
        let expect = case["expect"]
            .as_object()
            .expect("expectations of the case");

        ratchet.record(
            format!("{id}/file"),
            check_file(&bytes, expect, &block_types),
        );
        for block_type in &block_types {
            if let Some(golden) = expect.get(block_type) {
                let golden = golden.as_object().expect("expectation of a block");
                let item = format!("{id}/{block_type}");
                match load_block(&bytes, block_type, golden) {
                    Replay::Present(actual) => {
                        ratchet
                            .record_differences(item, compare_block(&tolerances, golden, &actual));
                    }
                    Replay::Absent => panic!("loading a block always has a result"),
                    Replay::Unimplemented => ratchet.record(item, Err(Failure::Unimplemented)),
                }
            }
        }
    }
    ratchet.finish();
}

/// Opens the show file and returns the name of the result: `SUCCESS`, or the
/// name of the error (`EPARSE`, `ECORRUPTED`, `EREAD`).
fn open_file(bytes: &[u8]) -> Replay {
    Replay::Present(json!(result_name(&BinaryFile::new(bytes))))
}

/// Loads the first block of the given type from the show file, and returns
/// its `result` and the properties that `golden` asks for, in the shape of
/// `golden` (see `cases.schema.json`). The time instants to query are the
/// ones in `golden`.
fn load_block(bytes: &[u8], block_type: &str, golden: &Map<String, Value>) -> Replay {
    let parser = match BinaryFile::new(bytes) {
        Ok(parser) => parser,
        Err(err) => return Replay::Present(json!({ "result": err.error_name() })),
    };
    match parser.find_first_block_by_type(binary_block_type(block_type)) {
        Ok(Some(block)) => load_body(block_type, block, golden),
        Ok(None) => Replay::Present(json!({ "result": "ENOENT" })),
        Err(err) => Replay::Present(json!({ "result": err.error_name() })),
    }
}

/// Parses the body of a block that was found in the show file, like
/// `load_block`.
fn load_body(block_type: &str, block: BinaryBlock<'_>, golden: &Map<String, Value>) -> Replay {
    match block_type {
        "trajectory" => Replay::Present(load_trajectory(block.body, golden)),
        "light_program" => Replay::Present(load_light_program(block.body, golden)),
        "event_list" => Replay::Present(load_event_list(block.body)),
        "yaw_control" => Replay::Present(load_yaw_control(block.body, golden)),
        "gcs_light_control_setup" => {
            Replay::Present(match GcsLightControlSetup::from_buffer(block.body) {
                Ok(setup) => {
                    let mut result = Map::new();
                    result.insert("result".into(), json!("SUCCESS"));
                    result.extend(values::gcs_light_control_setup(&setup));
                    Value::Object(result)
                }
                Err(err) => json!({ "result": err.error_name() }),
            })
        }
        "rth_plan" => Replay::Present(match RthPlan::from_buffer(block.body) {
            Ok(plan) => {
                let points: Vec<Value> = (0..plan.num_points())
                    .filter_map(|index| plan.point(index))
                    .map(|point| json!([point.x, point.y]))
                    .collect();
                json!({ "result": "SUCCESS", "points_mm": points, "num_entries": plan.num_entries() })
            }
            Err(err) => json!({ "result": err.error_name() }),
        }),
        "metadata" => Replay::Present(match ShowMetadata::from_buffer(block.body) {
            Ok(metadata) => {
                let mut result = Map::new();
                result.insert("result".into(), json!("SUCCESS"));
                result.extend(values::metadata(metadata));
                Value::Object(result)
            }
            Err(err) => json!({ "result": err.error_name() }),
        }),
        _ => Replay::Unimplemented,
    }
}

fn load_trajectory(body: &[u8], golden: &Map<String, Value>) -> Value {
    let trajectory = match Trajectory::from_buffer(body) {
        Ok(trajectory) => trajectory,
        Err(err) => return json!({ "result": err.error_name() }),
    };
    let mut result = Map::new();
    result.insert("result".into(), json!("SUCCESS"));
    if golden.contains_key("duration_ms") {
        result.insert(
            "duration_ms".into(),
            json!(trajectory.total_duration_msec()),
        );
    }
    if let Some(positions) = golden.get("positions").and_then(Value::as_array) {
        let mut player = TrajectoryPlayer::new(&trajectory);
        let rows = positions.iter().map(|row| {
            let time_ms = values::u32_of(&row[0]);
            let position = player.position_at(values::msec_to_sec(time_ms));
            json!([time_ms, position.x, position.y, position.z])
        });
        result.insert("positions".into(), rows.collect());
    }
    Value::Object(result)
}

/// How long the C conformance test lets a light program run before it
/// concludes that it never ends, in milliseconds.
const LIGHT_PROGRAM_HORIZON_MSEC: u64 = 10_000_000;

fn load_light_program(body: &[u8], golden: &Map<String, Value>) -> Value {
    let program = LightProgram::from_buffer(body);
    let mut result = Map::new();
    result.insert("result".into(), json!("SUCCESS"));
    if let Some(colors) = golden.get("colors").and_then(Value::as_array) {
        let times: Vec<u64> = colors
            .iter()
            .map(|row| u64::from(values::u32_of(&row[0])))
            .collect();
        let mut player = LightPlayer::new(&program);
        let rows: Vec<Value> = times
            .iter()
            .map(|&t| {
                let color = player.color_at(t);
                json!([t, color.red, color.green, color.blue])
            })
            .collect();
        // The colors must not depend on the order of the queries; a
        // difference shows up as an unexpected key
        let reversed: Vec<Value> = times
            .iter()
            .rev()
            .map(|&t| {
                let color = player.color_at(t);
                json!([t, color.red, color.green, color.blue])
            })
            .collect();
        if reversed.iter().rev().ne(rows.iter()) {
            result.insert("colors_in_reverse_order".into(), reversed.into());
        }
        result.insert("colors".into(), rows.into());
    }
    if golden.contains_key("ends_at_ms") {
        let end = lights::end_msec(&program, LIGHT_PROGRAM_HORIZON_MSEC);
        result.insert("ends_at_ms".into(), json!(end));
    }
    Value::Object(result)
}

fn load_yaw_control(body: &[u8], golden: &Map<String, Value>) -> Value {
    let ctrl = match YawControl::from_buffer(body) {
        Ok(ctrl) => ctrl,
        Err(err) => return json!({ "result": err.error_name() }),
    };
    let mut result = Map::new();
    result.insert("result".into(), json!("SUCCESS"));
    result.insert("auto_yaw".into(), json!(ctrl.auto_yaw()));
    result.insert("yaw_offset_ddeg".into(), json!(ctrl.yaw_offset_ddeg()));
    result.insert("num_deltas".into(), json!(ctrl.num_deltas()));
    if let Some(rows) = golden.get("yaw_deg").and_then(Value::as_array) {
        let mut player = YawPlayer::new(&ctrl);
        let rows: Vec<Value> = rows
            .iter()
            .map(|row| {
                let msec = values::u32_of(&row[0]);
                json!([msec, player.yaw_at(values::msec_to_sec(msec))])
            })
            .collect();
        result.insert("yaw_deg".into(), rows.into());
    }
    Value::Object(result)
}

fn load_event_list(body: &[u8]) -> Value {
    match EventList::from_buffer(body) {
        Ok(list) => json!({ "result": "SUCCESS", "events": events::events(&list) }),
        Err(err) => json!({ "result": err.error_name() }),
    }
}

/// The type of the blocks of a block type of the cases.
fn binary_block_type(block_type: &str) -> BinaryBlockType {
    match block_type {
        "trajectory" => BinaryBlockType::TRAJECTORY,
        "light_program" => BinaryBlockType::LIGHT_PROGRAM,
        "yaw_control" => BinaryBlockType::YAW_CONTROL,
        "event_list" => BinaryBlockType::EVENT_LIST,
        "rth_plan" => BinaryBlockType::RTH_PLAN,
        "gcs_light_control_setup" => BinaryBlockType::GCS_LIGHT_CONTROL_SETUP,
        "metadata" => BinaryBlockType::SHOW_METADATA,
        _ => panic!("unknown block type in the cases: {block_type}"),
    }
}

/// Checks the result of opening the file, and that the block types not listed
/// in the case are reported as missing.
fn check_file(
    bytes: &[u8],
    expect: &Map<String, Value>,
    block_types: &[String],
) -> Result<(), Failure> {
    let actual = match open_file(bytes) {
        Replay::Present(actual) => actual,
        Replay::Absent => panic!("opening a file always has a result"),
        Replay::Unimplemented => return Err(Failure::Unimplemented),
    };
    let mut differences = Vec::new();
    if actual != expect["file"] {
        differences.push(format!(".file: expected {}, got {actual}", expect["file"]));
    } else if actual == "SUCCESS" {
        for block_type in block_types
            .iter()
            .filter(|block_type| !expect.contains_key(*block_type))
        {
            let missing = json!({ "result": "ENOENT" });
            match load_block(bytes, block_type, missing.as_object().expect("object")) {
                Replay::Present(actual) if actual.get("result") == Some(&missing["result"]) => {}
                Replay::Present(actual) => differences.push(format!(
                    ".{block_type}: expected the block to be missing, got {}",
                    actual.get("result").unwrap_or(&Value::Null)
                )),
                Replay::Absent => panic!("loading a block always has a result"),
                Replay::Unimplemented => return Err(Failure::Unimplemented),
            }
        }
    }
    if differences.is_empty() {
        Ok(())
    } else {
        Err(Failure::Differences(differences))
    }
}

/// Compares the properties of a block that the case lists; the other
/// properties are not checked.
fn compare_block(
    tolerances: &Tolerances,
    golden: &Map<String, Value>,
    actual: &Value,
) -> Vec<String> {
    let Some(actual) = actual.as_object() else {
        return vec![format!("expected an object, got {actual}")];
    };
    let actual: Map<String, Value> = actual
        .iter()
        .filter(|(key, _)| golden.contains_key(*key))
        .map(|(key, value)| (key.clone(), name_columns(key, value)))
        .collect();
    let golden: Map<String, Value> = golden
        .iter()
        .map(|(key, value)| (key.clone(), name_columns(key, value)))
        .collect();

    let mut comparator = Comparator::new(tolerances);
    comparator.compare_root(&Value::Object(golden), &Value::Object(actual));
    comparator.into_differences()
}

/// Turns the tuples of a property into objects with named columns, so that
/// the comparator applies the tolerance of each column.
fn name_columns(key: &str, value: &Value) -> Value {
    let Some((_, columns)) = COLUMNS.iter().find(|(name, _)| *name == key) else {
        return value.clone();
    };
    let Some(rows) = value.as_array() else {
        return value.clone();
    };
    let rows = rows.iter().map(|row| match row.as_array() {
        Some(fields) if fields.len() == columns.len() => Value::Object(
            columns
                .iter()
                .zip(fields)
                .map(|(column, field)| ((*column).to_owned(), field.clone()))
                .collect(),
        ),
        // Left as it is; the comparison reports the difference in shape.
        _ => row.clone(),
    });
    Value::Array(rows.collect())
}

/// The tolerances at the top of `cases.json`: positions in millimeters and
/// yaw angles in degrees, absolute. Everything else must match exactly.
fn tolerances(value: &Value) -> Tolerances {
    let value = value.as_object().expect("tolerances of the cases");
    let mut names: Vec<&str> = value.keys().map(String::as_str).collect();
    names.sort_unstable();
    assert_eq!(
        names,
        ["position_mm", "yaw_deg"],
        "unknown tolerances in cases.json"
    );
    let absolute = |name: &str| Tolerance {
        abs: value[name].as_f64().expect("tolerance"),
        rel: 0.0,
    };
    Tolerances::new(
        [
            ("mm".to_owned(), absolute("position_mm")),
            ("deg".to_owned(), absolute("yaw_deg")),
        ],
        Tolerance::EXACT,
    )
}

/// The block types that a case may list, from the schema of the cases.
fn block_types() -> Vec<String> {
    let schema = oracle::read_json("conformance/cases.schema.json");
    let properties = schema["$defs"]["expectation"]["properties"]
        .as_object()
        .expect("properties of an expectation in the schema");
    properties
        .keys()
        .filter(|key| *key != "file")
        .cloned()
        .collect()
}
