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

#[path = "api_traces/builder.rs"]
mod builder;
#[path = "api_traces/colors.rs"]
mod colors;
mod common;
#[path = "api_traces/edit.rs"]
mod edit;
#[path = "api_traces/events.rs"]
mod events;
#[path = "api_traces/lights.rs"]
mod lights;
#[path = "api_traces/motion.rs"]
mod motion;
#[path = "api_traces/rth_plan.rs"]
mod rth_plan;
#[path = "api_traces/screenplay.rs"]
mod screenplay;
#[path = "api_traces/stats.rs"]
mod stats;
#[path = "api_traces/time_axis.rs"]
mod time_axis;
#[path = "api_traces/trajectory.rs"]
mod trajectory;
#[path = "api_traces/yaw_control.rs"]
mod yaw_control;

use common::ratchet::Ratchet;
use common::{Replay, oracle, record_section};
use serde_json::{Map, Value};
use skybrush::events::EventList;
use skybrush::formats::binary::BinaryFile;
use skybrush::lights::LightProgram;
use skybrush::rth_plan::RthPlan;
use skybrush::trajectory::Trajectory;

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
fn replay_section(section: &str, show: &[u8], golden: &Map<String, Value>) -> Replay {
    let file = BinaryFile::new(show).expect("the show files of the traces are valid");
    let trajectory = || Trajectory::from_binary_file(&file).expect("valid trajectory");
    match section {
        "trajectory" => trajectory::replay(trajectory().as_ref()),
        "stats" => stats::replay_stats(trajectory().as_ref(), &golden[section]),
        "proposed_takeoff_times" => {
            stats::replay_proposed_takeoff_times(trajectory().as_ref(), &golden[section])
        }
        "proposed_landing_times" => {
            stats::replay_proposed_landing_times(trajectory().as_ref(), &golden[section])
        }
        "cuts" => edit::replay_cuts(trajectory().as_ref(), &golden[section]),
        "landings" => edit::replay_landings(trajectory().as_ref(), &golden[section]),
        "cleared_view" => edit::replay_cleared_view(trajectory()),
        "light_player" => {
            let program = LightProgram::from_binary_file(&file).expect("valid light program");
            lights::replay_light_player(program.as_ref())
        }
        "rth_plan_queries" => {
            let plan = RthPlan::from_binary_file(&file).expect("valid RTH plan");
            rth_plan::replay_queries(plan.filter(|_| trajectory().is_some()))
        }
        "rth_trajectories" => {
            let plan = RthPlan::from_binary_file(&file).expect("valid RTH plan");
            rth_plan::replay_trajectories(trajectory().as_ref(), plan.as_ref())
        }
        "event_player" => {
            let list = EventList::from_binary_file(&file).expect("valid event list");
            events::replay_event_player(list, golden.get(section))
        }
        _ => Replay::Unimplemented,
    }
}

/// Computes a section of `scenarios.json`. The inputs of the computations are
/// recorded in the golden section next to their results.
fn replay_scenarios(section: &str, golden: &Map<String, Value>) -> Replay {
    match section {
        "colors" => colors::replay(&golden[section]),
        "motion" => motion::replay(&golden[section]),
        "light_programs" => lights::replay_scenarios(&golden[section]),
        "builder" => builder::replay(&golden[section]),
        "stats_scenarios" => stats::replay_scenarios(&golden[section]),
        "bounding_boxes" => trajectory::replay_bounding_boxes(&golden[section]),
        "event_lists" => events::replay_scenarios(&golden[section]),
        "rth_conversions" => rth_plan::replay_conversions(&golden[section]),
        "empty_rth_plan" => rth_plan::replay_empty_plan(),
        "screenplay_queries" => screenplay::replay_queries(&golden[section]),
        "time_axes" => time_axis::replay_time_axes(&golden[section]),
        "time_axis_editing" => time_axis::replay_editing(&golden[section]),
        "constant_yaw" => yaw_control::replay_constant_yaw(&golden[section]),
        _ => Replay::Unimplemented,
    }
}
