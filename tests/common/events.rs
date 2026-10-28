//! Events as the oracle records them: `[time_msec, type, subtype, payload]`,
//! with the payload in hexadecimal.

use serde_json::{Value, json};
use skybrush::events::{Event, EventList, EventType};

use super::oracle::{decode_hex, encode_hex};
use super::values::u32_of;

pub fn event(event: &Event) -> Value {
    json!([
        event.time_msec,
        event.event_type.0,
        event.subtype,
        encode_hex(&event.payload)
    ])
}

pub fn events(events: &EventList) -> Value {
    events.iter().map(event).collect()
}

pub fn event_of(value: &Value) -> Event {
    let byte =
        |index: usize| u8::try_from(value[index].as_u64().expect("an integer")).expect("a byte");
    let payload = decode_hex(value[3].as_str().expect("the payload in hex"));
    Event {
        time_msec: u32_of(&value[0]),
        event_type: EventType(byte(1)),
        subtype: byte(2),
        payload: payload.try_into().expect("a payload of four bytes"),
    }
}
