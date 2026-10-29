//! Fuzzes whole show files: every component, and the show controller.
#![no_main]

use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| {
    skybrush_fuzz::show_file(data);
    if let Some(data) = skybrush_fuzz::as_version_1(data) {
        skybrush_fuzz::show_file(&data);
    }
});
