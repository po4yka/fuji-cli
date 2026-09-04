#![no_main]

use fujicli::{decode_exact, encode, DeviceInfo};
use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| {
    // `DeviceInfo` is a fixed-layout binrw struct whose variable-length
    // parts are PTP strings and PTP arrays, so on any input that decodes
    // exactly, the re-encoded bytes must be identical to the input. A
    // mismatch means the decoder and encoder disagree about the wire
    // format. Byte identity is a strictly stronger oracle than the
    // decode-encode-decode value equality used elsewhere, and it needs no
    // `PartialEq` derive on the production type.
    let Ok(info) = decode_exact::<DeviceInfo>(data) else {
        return;
    };
    let re_encoded = encode(&info).expect("a decoded DeviceInfo must always re-encode");
    assert_eq!(
        data, re_encoded,
        "DeviceInfo is not byte-identical across decode and re-encode"
    );
});
