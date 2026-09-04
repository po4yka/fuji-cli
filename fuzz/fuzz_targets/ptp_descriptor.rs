#![no_main]

use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| {
    // A camera answer must never panic, hang, or abort the process. The
    // descriptor parser is hand-rolled over fully untrusted bytes, so every
    // rejection path (datatype, form, allocation budget, trailing bytes)
    // must stay a `Result`, never a panic.
    let _ = fujicli::decode_device_prop_desc_for_fuzzing(data);
});
