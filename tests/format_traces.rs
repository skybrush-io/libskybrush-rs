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
#[path = "format_traces/stats.rs"]
mod stats;
#[path = "format_traces/trajectory.rs"]
mod trajectory;

use common::ratchet::Ratchet;
use common::{Replay, oracle, record_section};
use serde_json::{Map, Value};
use skybrush::formats::binary::BinaryFile;
use skybrush::trajectory::Trajectory;

/// Keys of a trace that are inputs or informational, not computed.
const HEADER: &[&str] = &[
    "$schema",
    "version",
    "fixture",
    "generator",
    "sample_interval_ms",
];

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
        "stats" => {
            let trajectory = Trajectory::from_binary_file(&file).expect("valid trajectory");
            stats::replay(trajectory.as_ref())
        }
        _ => Replay::Unimplemented,
    }
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
