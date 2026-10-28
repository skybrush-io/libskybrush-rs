//! Tests of the comparison of traces (`tests/common/compare.rs`).
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)]

mod common;

use std::{env, fs};

use common::compare::{Comparator, Tolerances};
use common::oracle;
use serde_json::{Value, json};

fn api_tolerances() -> Tolerances {
    oracle::read_tolerances("api")
}

fn differences(expected: &Value, actual: &Value) -> Vec<String> {
    let tolerances = api_tolerances();
    let mut comparator = Comparator::new(&tolerances);
    comparator.compare_root(expected, actual);
    comparator.into_differences()
}

/// Asserts that a value matches the golden value, showing the differences if
/// it does not.
#[track_caller]
fn assert_matches(expected: &Value, actual: &Value) {
    assert_eq!(differences(expected, actual), Vec::<String>::new());
}

#[test]
fn golden_traces_match_themselves() {
    for (dir, except) in [
        ("format", &["tolerances", "trace.schema"][..]),
        ("api", &["tolerances"][..]),
    ] {
        let tolerances = oracle::read_tolerances(dir);
        for name in oracle::trace_names(dir, except) {
            let golden = oracle::read_json(&format!("{dir}/{name}.json"));
            let mut comparator = Comparator::new(&tolerances);
            comparator.compare_root(&golden, &golden);
            assert_eq!(
                comparator.into_differences(),
                Vec::<String>::new(),
                "{dir}/{name}"
            );
        }
    }
}

#[test]
fn tolerance_is_absolute_plus_relative() {
    // mm: abs 0.01, rel 1e-5, so 0.01 + 1e-5 * 1000 = 0.02 is allowed at 1000
    let golden = json!({ "x_mm": 1000.0 });
    assert_matches(&golden, &json!({ "x_mm": 1000.019 }));
    assert_matches(&golden, &json!({ "x_mm": 999.981 }));
    assert_eq!(
        differences(&golden, &json!({ "x_mm": 1000.021 })),
        [".x_mm: expected 1000.0, got 1000.021 (allowed difference: 0.02)"]
    );
}

#[test]
fn longest_suffix_wins() {
    // mm_s2 allows 0.1 + 1e-3 * 100 = 0.2; mm_s would allow only 0.02
    let golden = json!({ "a_mm_s2": 100.0 });
    assert_matches(&golden, &json!({ "a_mm_s2": 100.15 }));
    // A suffix only counts after an underscore: 0.01 would be within the
    // tolerance of mm (0.011) but not within the default one (0.0011)
    let golden = json!({ "summm": 100.0 });
    assert_eq!(differences(&golden, &json!({ "summm": 100.01 })).len(), 1);
    assert_matches(&json!({ "sum_mm": 100.0 }), &json!({ "sum_mm": 100.01 }));
}

#[test]
fn integers_without_unit_match_exactly() {
    assert_eq!(
        differences(&json!({ "count": 5 }), &json!({ "count": 6 })),
        [".count: expected 5, got 6"]
    );
    assert_eq!(
        differences(&json!({ "time_ms": 100 }), &json!({ "time_ms": 101 })).len(),
        1
    );
    // An integer with a unit is a number like any other
    assert_matches(&json!({ "x_units": 100 }), &json!({ "x_units": 101 }));
    assert_eq!(
        differences(&json!({ "x_units": 100 }), &json!({ "x_units": 102 })).len(),
        1
    );
    // A float on either side uses the default tolerance (abs 1e-4, rel 1e-5)
    assert_matches(&json!({ "count": 5 }), &json!({ "count": 5.0 }));
    assert_matches(&json!({ "ratio": 0.5 }), &json!({ "ratio": 0.500_01 }));
    assert_eq!(
        differences(&json!({ "ratio": 0.5 }), &json!({ "ratio": 0.501 })).len(),
        1
    );
}

#[test]
fn array_items_inherit_the_unit_of_the_array() {
    let golden = json!({ "position_mm": [[1.0, 2.0], [3.0, 4.0]] });
    assert_matches(
        &golden,
        &json!({ "position_mm": [[1.005, 2.0], [3.0, 4.005]] }),
    );
    assert_eq!(
        differences(&golden, &json!({ "position_mm": [[1.0, 2.0], [3.0, 4.5]] })),
        [".position_mm[1][1]: expected 4.0, got 4.5 (allowed difference: 0.01004)"]
    );
    // Objects within the array use their own keys: 0.01 would be within the
    // tolerance of mm (0.011) but not within the default one (0.0011)
    let golden = json!({ "samples_mm": [{ "time_ms": 100 }] });
    assert_eq!(
        differences(&golden, &json!({ "samples_mm": [{ "time_ms": 100.01 }] })).len(),
        1
    );
}

#[test]
fn rth_plan_entries_use_named_columns() {
    let golden = json!({
        "entry_fields": ["time_ms", "target_mm", "duration_sec"],
        "entries": [[1000, 5000.0, 2.0]]
    });
    // target_mm allows 0.01 + 0.05, duration_sec allows 0.001, time_ms is exact
    let actual = json!({
        "entry_fields": ["time_ms", "target_mm", "duration_sec"],
        "entries": [[1000, 5000.05, 2.0005]]
    });
    assert_matches(&golden, &actual);
    let actual = json!({
        "entry_fields": ["time_ms", "target_mm", "duration_sec"],
        "entries": [[1001, 5000.0, 2.0]]
    });
    assert_eq!(
        differences(&golden, &actual),
        [".entries[0].time_ms: expected 1000, got 1001"]
    );
    let actual = json!({
        "entry_fields": ["time_ms", "target_mm", "duration_sec"],
        "entries": [[1000, 5000.0]]
    });
    assert_eq!(
        differences(&golden, &actual),
        [".entries[0]: expected 3 fields, got 2"]
    );
}

#[test]
fn booleans_and_nulls_match_exactly() {
    assert_eq!(
        differences(&json!({ "ended": true }), &json!({ "ended": 1 })).len(),
        1
    );
    assert_eq!(
        differences(&json!({ "x": 0 }), &json!({ "x": false })).len(),
        1
    );
    assert_matches(&json!({ "x_mm": null }), &json!({ "x_mm": null }));
    assert_eq!(
        differences(&json!({ "x_mm": null }), &json!({ "x_mm": 0.0 })),
        [".x_mm: expected null, got 0.0"]
    );
}

#[test]
fn keys_must_agree_except_the_generator() {
    let golden = json!({ "a": 1, "b": { "c": 2 }, "generator": "libskybrush 6.0.1" });
    let actual = json!({ "b": { "c": 2, "d": 3 }, "generator": "skybrush-rs" });
    assert_eq!(
        differences(&golden, &actual),
        [
            ".a: missing from the trace",
            ".b.d: not present in the golden trace"
        ]
    );
}

#[test]
fn arrays_and_strings() {
    assert_eq!(
        differences(&json!({ "xs": [1, 2] }), &json!({ "xs": [1] })),
        [".xs: expected 2 items, got 1"]
    );
    assert_eq!(
        differences(
            &json!({ "result": "SUCCESS" }),
            &json!({ "result": "EPARSE" })
        ),
        [r#".result: expected "SUCCESS", got "EPARSE""#]
    );
    assert_eq!(differences(&json!([1]), &json!({ "a": 1 })).len(), 1);
    assert_eq!(differences(&json!(1), &json!(2)), [".: expected 1, got 2"]);
}

#[test]
fn sections_compare_like_whole_traces() {
    let tolerances = api_tolerances();
    let golden = json!({
        "entry_fields": ["time_ms"],
        "entries": [[1000]],
        "stats_mm": [1.0]
    });
    let golden = golden.as_object().unwrap();
    let mut comparator = Comparator::new(&tolerances);
    comparator.compare_member(golden, "entries", &json!([[1001]]));
    comparator.compare_member(golden, "stats_mm", &json!([1.5]));
    assert_eq!(
        comparator.into_differences(),
        [
            ".entries[0].time_ms: expected 1000, got 1001",
            ".stats_mm[0]: expected 1.0, got 1.5 (allowed difference: 0.01001)"
        ]
    );
}

/// Compares two trace files with the tolerances of a directory of the oracle,
/// e.g. to cross-check the comparator with `compare_traces.py`:
///
/// ```text
/// COMPARE_TOLERANCES=api COMPARE_EXPECTED=a.json COMPARE_ACTUAL=b.json \
///     cargo test --test comparator compare_files -- --ignored --nocapture
/// ```
#[test]
#[ignore = "a tool, run on demand"]
fn compare_files() {
    let var = |name: &str| env::var(name).unwrap_or_else(|_| panic!("{name} is not set"));
    let read = |path: String| -> Value {
        serde_json::from_slice(&fs::read(&path).unwrap_or_else(|err| panic!("{path}: {err}")))
            .unwrap_or_else(|err| panic!("{path}: {err}"))
    };
    let tolerances = oracle::read_tolerances(&var("COMPARE_TOLERANCES"));
    let mut comparator = Comparator::new(&tolerances);
    comparator.compare_root(&read(var("COMPARE_EXPECTED")), &read(var("COMPARE_ACTUAL")));
    for difference in comparator.into_differences() {
        println!("{difference}");
    }
}
