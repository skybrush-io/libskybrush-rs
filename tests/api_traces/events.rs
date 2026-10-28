//! Replays the `event_player` section of an API trace and the `event_lists`
//! section of `scenarios.json`: modifications of event lists, and the events
//! that the event player fires.

use serde_json::{Map, Value, json};
use skybrush::events::{EventList, EventListPlayer, EventType};

use crate::common::Replay;
use crate::common::errors::result_name;
use crate::common::events::{event_of, events};
use crate::common::values::u32_of;

/// The times of the queries of the event player in the scenarios, in
/// seconds. The oracle records the non-finite ones as `null`, so they are
/// listed here; the replay records them, so the comparison checks them.
const QUERY_TIMES_SEC: [f32; 12] = [
    -1.0,
    0.0,
    0.5,
    1.0,
    2.5,
    3.0,
    3.5,
    4_292_000.0,
    4_294_966.0,
    4_294_967.5,
    f32::INFINITY,
    f32::NAN,
];

/// The index of the next event after seeking the player to each sample time,
/// up to the end of the trace. The number of samples comes from the golden
/// trace; without one, the section is present but empty.
pub fn replay_event_player(list: Option<EventList>, golden: Option<&Value>) -> Replay {
    let Some(list) = list else {
        return Replay::Absent;
    };
    let num_samples = golden.map_or(0, |golden| {
        golden["next_event_index_after_seek"]
            .as_array()
            .expect("indices")
            .len()
    });
    let mut player = EventListPlayer::new(&list);
    let indices: Vec<Value> = (0..num_samples)
        .map(|i| {
            let t_msec = u32::try_from(i).expect("a small index") * 100;
            #[expect(clippy::cast_precision_loss, reason = "as in the generator")]
            player.seek(t_msec as f32 / 1000.0);
            json!(player.peek_next_event().map(|_| player.current_index()))
        })
        .collect();
    Replay::Present(json!({ "next_event_index_after_seek": indices }))
}

pub fn replay_scenarios(golden: &Value) -> Replay {
    let scenarios = golden.as_array().expect("scenarios");
    Replay::Present(scenarios.iter().map(replay_scenario).collect())
}

fn replay_scenario(golden: &Value) -> Value {
    let mut list = EventList::new();
    let ops: Vec<Value> = golden["ops"]
        .as_array()
        .expect("ops")
        .iter()
        .map(|op| replay_op(&mut list, op))
        .collect();

    if !list.is_sorted() {
        list.sort();
    }
    let mut player = EventListPlayer::new(&list);
    let mut next_events = Vec::new();
    loop {
        let index = player.current_index();
        if player.next_event().is_none() {
            break;
        }
        next_events.push(index);
    }

    player.rewind();
    let not_later_than: Vec<Value> = QUERY_TIMES_SEC
        .iter()
        .map(|&t| {
            let index = player.current_index();
            json!(player.next_event_not_later_than(t).map(|_| index))
        })
        .collect();
    let peek_after_seek: Vec<Value> = QUERY_TIMES_SEC
        .iter()
        .map(|&t| {
            player.seek(t);
            json!(player.peek_next_event().map(|_| player.current_index()))
        })
        .collect();

    json!({
        "scenario": golden["scenario"],
        "ops": ops,
        "sorted_for_playback": Value::Object(state(&list)),
        "next_events": next_events,
        "query_times_sec": QUERY_TIMES_SEC,
        "next_event_not_later_than": not_later_than,
        "peek_after_seek": peek_after_seek,
    })
}

fn replay_op(list: &mut EventList, golden: &Value) -> Value {
    let op = golden["op"].as_str().expect("op");
    let mut result = Map::new();
    result.insert("op".into(), json!(op));
    let outcome = match op {
        "append" | "insert" => {
            let event = event_of(&golden["event"]);
            result.insert("event".into(), golden["event"].clone());
            if op == "append" {
                result_name(&list.append(event))
            } else {
                result_name(&list.insert(event))
            }
        }
        "remove" => {
            let index = golden["index"].as_u64().expect("index");
            result.insert("index".into(), json!(index));
            match list.remove(usize::try_from(index).expect("an index")) {
                Some(_) => "SUCCESS",
                None => "EINVAL",
            }
        }
        "sort" => {
            list.sort();
            "SUCCESS"
        }
        "add_pyro_off_events" => {
            let duration = u32_of(&golden["duration_ms"]);
            result.insert("duration_ms".into(), json!(duration));
            result_name(&list.add_pyro_off_events(duration))
        }
        "adjust_timestamps_by_type" => {
            let event_type = golden["type"].as_u64().expect("type");
            let delta = golden["delta_ms"].as_i64().expect("delta");
            result.insert("type".into(), json!(event_type));
            result.insert("delta_ms".into(), json!(delta));
            list.adjust_timestamps_by_type(
                EventType(u8::try_from(event_type).expect("a byte")),
                i32::try_from(delta).expect("a 32-bit delta"),
            );
            "SUCCESS"
        }
        _ => panic!("unknown operation on an event list: {op}"),
    };
    result.insert("result".into(), json!(outcome));
    result.extend(state(list));
    Value::Object(result)
}

fn state(list: &EventList) -> Map<String, Value> {
    let mut result = Map::new();
    result.insert("events".into(), events(list));
    result.insert("is_sorted".into(), json!(list.is_sorted()));
    result.insert("is_empty".into(), json!(list.is_empty()));
    result
}
