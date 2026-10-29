//! Fuzzes the body of a yaw control block.
#![no_main]

use libfuzzer_sys::fuzz_target;
use skybrush::yaw_control::YawControl;

fuzz_target!(|data: &[u8]| {
    if let Ok(ctrl) = YawControl::from_buffer(data) {
        skybrush_fuzz::yaw_control(&ctrl);
    }
});
