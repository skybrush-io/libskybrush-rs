//! Shared parts of the test harness that replays the test oracle.
//!
//! Not every test binary uses every part.
#![allow(dead_code)]

pub mod compare;
pub mod errors;
pub mod lights;
pub mod oracle;
pub mod ratchet;
pub mod values;

use compare::{Comparator, Tolerances};
use ratchet::{Failure, Ratchet};
use serde_json::{Map, Value};

/// What the crate computed for an item of the oracle.
#[derive(Debug)]
pub enum Replay {
    /// The crate cannot compute the item yet.
    Unimplemented,
    /// The item does not exist, e.g. the show file has no such component.
    Absent,
    /// The item, in the shape of the golden data.
    Present(Value),
}

/// Records the result of replaying the section of a trace. A section that is
/// absent from the golden trace must be absent from the replay as well.
pub fn record_section(
    ratchet: &mut Ratchet,
    item: String,
    tolerances: &Tolerances,
    golden: &Map<String, Value>,
    section: &str,
    replay: Replay,
) {
    let differences = match (golden.contains_key(section), replay) {
        (_, Replay::Unimplemented) => return ratchet.record(item, Err(Failure::Unimplemented)),
        (true, Replay::Present(actual)) => {
            let mut comparator = Comparator::new(tolerances);
            comparator.compare_member(golden, section, &actual);
            comparator.into_differences()
        }
        (true, Replay::Absent) => vec![format!(".{section}: missing from the trace")],
        (false, Replay::Present(_)) => vec![format!(".{section}: not present in the golden trace")],
        (false, Replay::Absent) => Vec::new(),
    };
    ratchet.record_differences(item, differences);
}

/// Splits the keys of a golden trace into header keys, which are inputs or
/// informational, and sections, which are computed. Panics on keys that are
/// neither, since the harness would silently skip them.
pub fn sections<'a>(
    name: &str,
    golden: &'a Map<String, Value>,
    header: &[&str],
    known: &[String],
) -> Vec<&'a str> {
    let mut sections = Vec::new();
    for key in golden.keys() {
        if header.contains(&key.as_str()) {
            continue;
        }
        assert!(known.contains(key), "{name}: unknown section {key}");
        sections.push(key.as_str());
    }
    sections
}
