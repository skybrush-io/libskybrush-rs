//! Fuzzes the body of a light program block.
#![no_main]

use libfuzzer_sys::fuzz_target;
use skybrush::lights::LightProgram;

fuzz_target!(|data: &[u8]| {
    skybrush_fuzz::light_program(&LightProgram::from_buffer(data));
});
