//! Fuzzes the bodies of the blocks that are tag-length-value streams: the
//! setup of light control from the ground station, and the show metadata.
#![no_main]

use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| {
    skybrush_fuzz::tlv(data);
});
