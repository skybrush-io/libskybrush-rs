//! Fuzzes the body of a trajectory block.
#![no_main]

use libfuzzer_sys::fuzz_target;
use skybrush::trajectory::Trajectory;

fuzz_target!(|data: &[u8]| {
    if let Ok(trajectory) = Trajectory::from_buffer(data) {
        skybrush_fuzz::trajectory(&trajectory);
    }
});
