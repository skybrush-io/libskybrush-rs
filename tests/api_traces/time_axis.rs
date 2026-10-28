//! Replays the `time_axes` and `time_axis_editing` sections of
//! `scenarios.json`: time axes built from segments, edited, and mapped
//! between the wall clock and warped time.

use serde_json::{Map, Value, json};
use skybrush::time_axis::{TimeAxis, TimeSegment};

use crate::common::Replay;
use crate::common::errors::result_name;
use crate::common::values::{f32_of, to_f32};

/// The interval of the samples of the mappings, and the margin before the
/// origin and after the end of an axis, in milliseconds.
const SAMPLE_INTERVAL_MSEC: usize = 100;
const MARGIN_MSEC: i32 = 1000;

/// How long an infinite axis is sampled after its origin, in milliseconds.
const MAX_SAMPLED_DURATION_MSEC: u32 = 60_000;

pub fn replay_time_axes(golden: &Value) -> Replay {
    let scenarios = golden.as_array().expect("scenarios");
    Replay::Present(
        scenarios
            .iter()
            .map(|scenario| {
                let mut axis = TimeAxis::new();
                let origin_msec = i32_of(&scenario["origin_ms"]);
                let mut result = Map::new();
                result.insert("scenario".into(), scenario["scenario"].clone());
                result.insert("origin_ms".into(), json!(origin_msec));
                result.insert(
                    "set_origin_result".into(),
                    json!(result_name(&axis.set_origin_msec(origin_msec))),
                );
                result.insert("resulting_origin_ms".into(), json!(axis.origin_msec()));
                let segments: Vec<Value> = scenario["segments"]
                    .as_array()
                    .expect("segments")
                    .iter()
                    .map(|spec| {
                        let (mut item, segment) = segment_of(spec);
                        let outcome = segment.map_or("EINVAL", |segment| {
                            result_name(&axis.append_segment(segment))
                        });
                        item.insert("append_result".into(), json!(outcome));
                        Value::Object(item)
                    })
                    .collect();
                result.insert("segments".into(), segments.into());
                result.extend(mapping(&axis));
                Value::Object(result)
            })
            .collect(),
    )
}

pub fn replay_editing(golden: &Value) -> Replay {
    let scenarios = golden.as_array().expect("scenarios");
    Replay::Present(
        scenarios
            .iter()
            .map(|scenario| {
                let mut axis = TimeAxis::new();
                let ops: Vec<Value> = scenario["ops"]
                    .as_array()
                    .expect("ops")
                    .iter()
                    .map(|op| replay_op(&mut axis, op))
                    .collect();
                let mut result = Map::new();
                result.insert("scenario".into(), scenario["scenario"].clone());
                result.insert("ops".into(), ops.into());
                result.extend(mapping(&axis));
                Value::Object(result)
            })
            .collect(),
    )
}

fn replay_op(axis: &mut TimeAxis, golden: &Value) -> Value {
    let op = golden["op"].as_str().expect("op");
    let mut result = Map::new();
    let outcome = match op {
        "set_origin_sec" => {
            let origin_sec = f32_of(&golden["origin_sec"]);
            result.insert("op".into(), json!(op));
            result.insert("origin_sec".into(), golden["origin_sec"].clone());
            result_name(&axis.set_origin_sec(origin_sec))
        }
        "remove_segment_at" => {
            let index = index_of(&golden["index"]);
            result.insert("op".into(), json!(op));
            result.insert("index".into(), json!(index));
            match axis.remove_segment_at(index) {
                Some(_) => "SUCCESS",
                None => "EINVAL",
            }
        }
        "append_segment" | "insert_segment_at" => {
            let (item, segment) = segment_of(golden);
            result.extend(item);
            result.insert("op".into(), json!(op));
            let index = (op == "insert_segment_at").then(|| index_of(&golden["index"]));
            if let Some(index) = index {
                result.insert("index".into(), json!(index));
            }
            let outcome = match (segment, index) {
                // The C library makes an invalid segment, which is rejected
                (None, _) => "EINVAL",
                (Some(segment), None) => result_name(&axis.append_segment(segment)),
                (Some(segment), Some(index)) => {
                    result_name(&axis.insert_segment_at(index, segment))
                }
            };
            // The invalid segment of the C library has a zero duration
            let made_duration = segment.map_or(0, |segment| duration_json(segment.duration_msec));
            result.insert("made_duration_ms".into(), json!(made_duration));
            outcome
        }
        _ => panic!("unknown operation on a time axis: {op}"),
    };
    result.insert("result".into(), json!(outcome));
    result.insert("origin_ms".into(), json!(axis.origin_msec()));
    Value::Object(result)
}

/// The segment that a specification of the golden data makes, and the
/// specification as the generator records it.
fn segment_of(spec: &Value) -> (Map<String, Value>, Option<TimeSegment>) {
    let initial_rate = rate_of(&spec["initial_rate"]);
    let final_rate = rate_of(&spec["final_rate"]);
    let mut item = Map::new();
    let segment = if let Some(warped) = spec.get("warped_duration_sec") {
        item.insert("warped_duration_sec".into(), warped.clone());
        TimeSegment::warped(f32_of(warped), initial_rate, final_rate)
    } else {
        let duration = spec["duration_ms"].as_i64().expect("duration");
        item.insert("duration_ms".into(), json!(duration));
        let duration_msec = u32::try_from(duration).unwrap_or(u32::MAX);
        Some(TimeSegment::new(duration_msec, initial_rate, final_rate))
    };
    item.insert("initial_rate".into(), json!(initial_rate));
    item.insert("final_rate".into(), json!(final_rate));
    (item, segment)
}

/// The segments of an axis, its durations, and its mapping between the wall
/// clock and warped time, from before its origin to after its end.
fn mapping(axis: &TimeAxis) -> Map<String, Value> {
    let origin_msec = axis.origin_msec();
    let duration_msec = axis.total_duration_msec();
    let total_warped = axis.total_warped_duration_sec();

    let mut result = Map::new();
    result.insert("num_segments".into(), json!(axis.num_segments()));
    let segments: Vec<Value> = axis
        .segments()
        .iter()
        .map(|segment| {
            json!([
                segment.duration_msec,
                segment.initial_rate,
                segment.final_rate
            ])
        })
        .collect();
    result.insert("resulting_segments".into(), segments.into());
    result.insert(
        "total_duration_ms".into(),
        json!(duration_json(duration_msec)),
    );
    result.insert(
        "total_duration_sec".into(),
        json!(axis.total_duration_sec()),
    );
    result.insert("total_warped_duration_sec".into(), json!(total_warped));

    // Infinite axes are sampled for a while after their origin only
    let sampled_msec = i32::try_from(duration_msec.min(MAX_SAMPLED_DURATION_MSEC)).expect("small");
    let from_msec = origin_msec - MARGIN_MSEC;
    let to_msec = origin_msec + sampled_msec + MARGIN_MSEC;
    let times = || (from_msec..=to_msec).step_by(SAMPLE_INTERVAL_MSEC);
    result.insert("map_from_ms".into(), json!(from_msec));
    let warped: Vec<f32> = times().map(|t| axis.map(t)).collect();
    result.insert("map_sec".into(), json!(warped));
    let rates: Vec<f32> = times().map(|t| axis.map_with_rate(t).1).collect();
    result.insert("map_rate".into(), json!(rates));

    let scale = if total_warped.is_finite() && total_warped > 0.0 {
        total_warped
    } else {
        10.0
    };
    let reverse: Vec<Value> = (0..=20_u8)
        .map(|j| {
            let warped = scale * (f32::from(j) / 20.0) * 1.2 - 1.0;
            json!([warped, axis.reverse_map(warped)])
        })
        .collect();
    result.insert("reverse_map".into(), reverse.into());
    result
}

/// An infinite duration is recorded as -1.
fn duration_json(duration_msec: u32) -> i64 {
    if duration_msec == u32::MAX {
        -1
    } else {
        i64::from(duration_msec)
    }
}

/// A rate of a segment. Rates that are not finite are recorded as `null`;
/// the scenarios use NaN only, to check that it is rejected.
fn rate_of(value: &Value) -> f32 {
    value.as_f64().map_or(f32::NAN, to_f32)
}

fn i32_of(value: &Value) -> i32 {
    i32::try_from(value.as_i64().expect("an integer")).expect("an i32")
}

fn index_of(value: &Value) -> usize {
    usize::try_from(value.as_u64().expect("an index")).expect("an index")
}
