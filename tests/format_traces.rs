//! Replays the golden traces of the `.skyb` show files
//! (`tests/oracle/format`).
//!
//! Each trace yields one item `<trace>/<section>` for every section that a
//! trace may have (see `trace.schema.json`); a section that is absent from the
//! golden trace must be absent from the replay as well.
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)]

mod common;
#[path = "format_traces/light_program.rs"]
mod light_program;
#[path = "format_traces/rth_plan.rs"]
mod rth_plan;
#[path = "format_traces/stats.rs"]
mod stats;
#[path = "format_traces/trajectory.rs"]
mod trajectory;
#[path = "format_traces/yaw_control.rs"]
mod yaw_control;

use common::ratchet::Ratchet;
use common::{Replay, events, oracle, record_section, values};
use serde_json::{Map, Value, json};
use skybrush::events::EventList;
use skybrush::formats::binary::BinaryFile;
use skybrush::gcs_light_control::GcsLightControlSetup;
use skybrush::lights::LightProgram;
use skybrush::metadata::ShowMetadata;
use skybrush::rth_plan::RthPlan;
use skybrush::trajectory::Trajectory;
use skybrush::yaw_control::YawControl;

/// Keys of a trace that are inputs or informational, not computed.
const HEADER: &[&str] = &[
    "$schema",
    "version",
    "fixture",
    "generator",
    "sample_interval_ms",
];

/// How long the generator samples after the end of the show, in
/// milliseconds.
const MARGIN_MSEC: u64 = 1000;

#[test]
fn format_traces() {
    let tolerances = oracle::read_tolerances("format");
    let sections = sections_of_schema();

    let mut ratchet = Ratchet::new("format_traces");
    for name in oracle::trace_names("format", &["tolerances", "trace.schema"]) {
        let golden = oracle::read_json(&format!("format/{name}.json"));
        oracle::assert_version(&name, &golden, 1);
        assert_eq!(
            golden["fixture"],
            format!("{name}.skyb"),
            "{name}: unexpected fixture"
        );
        let golden = golden.as_object().expect("trace");
        common::sections(&name, golden, HEADER, &sections);

        let show = oracle::read_bytes(&format!("format/{name}.skyb"));
        for section in &sections {
            let replay = replay_section(section, &show, golden);
            record_section(
                &mut ratchet,
                format!("{name}/{section}"),
                &tolerances,
                golden,
                section,
                replay,
            );
        }
    }
    ratchet.finish();
}

/// Computes a section of the trace of a show file, in the shape of the golden
/// trace (see `trace.schema.json`). The inputs of the computation, such as
/// `sample_interval_ms`, are read from the golden trace.
fn replay_section(section: &str, show: &[u8], golden: &Map<String, Value>) -> Replay {
    let file = BinaryFile::new(show).expect("the show files of the traces are valid");
    match section {
        "trajectory" => {
            let trajectory = Trajectory::from_binary_file(&file).expect("valid trajectory");
            trajectory::replay(trajectory.as_ref(), golden)
        }
        "light_program" => {
            let program = LightProgram::from_binary_file(&file).expect("valid light program");
            light_program::replay(program.as_ref(), golden)
        }
        "event_list" => {
            let list = EventList::from_binary_file(&file).expect("valid event list");
            match list {
                Some(list) => Replay::Present(json!({ "events": events::events(&list) })),
                None => Replay::Absent,
            }
        }
        "yaw_control" => {
            let ctrl = YawControl::from_binary_file(&file).expect("valid yaw control");
            yaw_control::replay(ctrl.as_ref(), golden)
        }
        "gcs_light_control_setup" => {
            let setup = GcsLightControlSetup::from_binary_file(&file).expect("valid setup");
            match setup {
                Some(setup) => {
                    Replay::Present(Value::Object(values::gcs_light_control_setup(&setup)))
                }
                None => Replay::Absent,
            }
        }
        "rth_plan" => {
            let plan = RthPlan::from_binary_file(&file).expect("valid RTH plan");
            rth_plan::replay(plan.as_ref(), golden)
        }
        "metadata" => {
            let metadata = ShowMetadata::from_binary_file(&file).expect("valid metadata");
            match metadata {
                Some(metadata) => Replay::Present(Value::Object(values::metadata(metadata))),
                None => Replay::Absent,
            }
        }
        "num_samples" => Replay::Present(json!(num_samples(&file, golden))),
        "stats" => {
            let trajectory = Trajectory::from_binary_file(&file).expect("valid trajectory");
            stats::replay(trajectory.as_ref())
        }
        _ => Replay::Unimplemented,
    }
}

/// The number of samples of the trace: the generator samples the longest
/// component of the show, and a margin after it.
fn num_samples(file: &BinaryFile<'_>, golden: &Map<String, Value>) -> u64 {
    let interval = golden["sample_interval_ms"]
        .as_u64()
        .expect("sample interval");
    let trajectory = Trajectory::from_binary_file(file).expect("valid trajectory");
    let yaw_control = YawControl::from_binary_file(file).expect("valid yaw control");
    let program = LightProgram::from_binary_file(file).expect("valid light program");
    let events = EventList::from_binary_file(file).expect("valid event list");
    let end_msec = [
        trajectory.map(|trajectory| u64::from(trajectory.total_duration_msec())),
        yaw_control.map(|ctrl| u64::from(ctrl.total_duration_msec())),
        program.and_then(|program| {
            common::lights::end_msec(&program, light_program::MAX_LIGHT_PROGRAM_DURATION_MSEC)
        }),
        events.and_then(|events| {
            events
                .as_slice()
                .last()
                .map(|event| u64::from(event.time_msec))
        }),
    ]
    .into_iter()
    .flatten()
    .max()
    .unwrap_or(0)
        + MARGIN_MSEC;
    end_msec.div_ceil(interval) + 1
}

/// The sections that a trace may have, from the schema of the traces.
fn sections_of_schema() -> Vec<String> {
    let schema = oracle::read_json("format/trace.schema.json");
    let properties = schema["properties"]
        .as_object()
        .expect("properties of a trace in the schema");
    properties
        .keys()
        .filter(|key| !HEADER.contains(&key.as_str()))
        .cloned()
        .collect()
}
