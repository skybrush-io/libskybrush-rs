//! Fuzzes the body of an event list block.
#![no_main]

use libfuzzer_sys::fuzz_target;
use skybrush::events::EventList;

fuzz_target!(|data: &[u8]| {
    if let Ok(events) = EventList::from_buffer(data) {
        skybrush_fuzz::event_list(events);
    }
});
