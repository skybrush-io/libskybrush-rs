//! Replays the golden traces of the API of the library (`tests/oracle/api`).
//!
//! Each trace of a show file yields one item `<trace>/<section>` for every
//! section that occurs in any of these traces; a section that is absent from
//! the golden trace must be absent from the replay as well. `scenarios.json`
//! yields one item `scenarios/<section>` for each of its sections.
//!
//! The show file of `<trace>.json` is `format/<trace>.skyb`, which is
//! identical to the fixture of libskybrush that the trace was generated from
//! (checked by `scripts/vendor-oracle.sh`).
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)]

mod common;

use common::ratchet::Ratchet;
use common::{Replay, oracle, record_section};
use serde_json::{Map, Value};

/// Keys of the trace of a show file that are inputs or informational. `end`
/// marks the end of the trace.
const HEADER: &[&str] = &[
    "version",
    "fixture",
    "generator",
    "sample_interval_ms",
    "end",
];

/// Keys of `scenarios.json` that are inputs or informational.
const SCENARIOS_HEADER: &[&str] = &["version", "generator", "sample_interval_ms"];

/// Results of the C library that have no counterpart in the crate, as
/// (section, key, reason). They are removed from the golden traces of the show
/// files before the comparison, so the replay must not produce them either.
const NOT_APPLICABLE: &[(&str, &str, &str)] = &[(
    "cleared_view",
    "bytes_changed_in_show_file",
    "clearing a trajectory in C writes through to the show file; in Rust, \
     borrowed data is immutable and the show file never changes",
)];

#[test]
fn api_traces() {
    let tolerances = oracle::read_tolerances("api");
    let names = oracle::trace_names("api", &["tolerances", "scenarios"]);
    let traces: Vec<(String, Map<String, Value>)> = names
        .into_iter()
        .map(|name| {
            let golden = oracle::read_json(&format!("api/{name}.json"));
            oracle::assert_version(&name, &golden, 1);
            assert_eq!(
                golden["fixture"],
                format!("{name}.skyb"),
                "{name}: unexpected fixture"
            );
            assert_eq!(golden["end"], true, "{name}: the trace is incomplete");
            let Value::Object(mut golden) = golden else {
                panic!("{name}: not an object")
            };
            remove_not_applicable(&name, &mut golden);
            (name, golden)
        })
        .collect();

    // A section of one trace must be checked in the others too, where the
    // replay must find it absent.
    let mut sections: Vec<String> = traces
        .iter()
        .flat_map(|(_, golden)| golden.keys())
        .filter(|key| !HEADER.contains(&key.as_str()))
        .cloned()
        .collect();
    sections.sort();
    sections.dedup();

    let mut ratchet = Ratchet::new("api_traces");
    for (name, golden) in &traces {
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

    let golden = oracle::read_json("api/scenarios.json");
    oracle::assert_version("scenarios", &golden, 1);
    let golden = golden.as_object().expect("scenarios");
    let all: Vec<String> = golden.keys().cloned().collect();
    for section in common::sections("scenarios", golden, SCENARIOS_HEADER, &all) {
        let replay = replay_scenarios(section, golden);
        record_section(
            &mut ratchet,
            format!("scenarios/{section}"),
            &tolerances,
            golden,
            section,
            replay,
        );
    }
    ratchet.finish();
}

/// Removes the results listed in `NOT_APPLICABLE` from the golden trace of a
/// show file. Panics if the trace has the section but not the key, since the
/// entry would then be stale.
fn remove_not_applicable(name: &str, golden: &mut Map<String, Value>) {
    for (section, key, _reason) in NOT_APPLICABLE {
        if let Some(section_value) = golden.get_mut(*section) {
            let removed = section_value
                .as_object_mut()
                .and_then(|object| object.remove(*key));
            assert!(
                removed.is_some(),
                "{name}: {section} has no {key}; remove it from NOT_APPLICABLE"
            );
        }
    }
}

/// Computes a section of the API trace of a show file, in the shape of the
/// golden trace (see `tests/oracle/api/README.md`). The inputs of the
/// computations are recorded in the golden section next to their results.
fn replay_section(_section: &str, _show: &[u8], _golden: &Map<String, Value>) -> Replay {
    Replay::Unimplemented
}

/// Computes a section of `scenarios.json`. The inputs of the computations are
/// recorded in the golden section next to their results.
fn replay_scenarios(_section: &str, _golden: &Map<String, Value>) -> Replay {
    Replay::Unimplemented
}
