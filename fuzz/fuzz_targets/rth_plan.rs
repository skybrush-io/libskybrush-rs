//! Fuzzes the body of an RTH plan block, and the trajectories of its entries.
#![no_main]

use libfuzzer_sys::fuzz_target;
use skybrush::rth_plan::RthPlan;

fuzz_target!(|data: &[u8]| {
    if let Ok(plan) = RthPlan::from_buffer(data) {
        skybrush_fuzz::rth_plan(&plan, None);
    }
});
