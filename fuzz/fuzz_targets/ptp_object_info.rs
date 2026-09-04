#![no_main]

use fujicli::{ObjectInfo, decode_exact, encode};
use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| {
    // `ObjectInfo` is a fixed-layout binrw struct whose only variable-length
    // parts are PTP strings, so on any input that decodes exactly, the
    // re-encoded bytes must be identical to the input. A mismatch means the
    // decoder and encoder disagree about the wire format. Byte identity is
    // a strictly stronger oracle than the decode-encode-decode value
    // equality used elsewhere, and it needs no `PartialEq` derive on the
    // production type.
    let Ok(info) = decode_exact::<ObjectInfo>(data) else {
        return;
    };
    let re_encoded = encode(&info).expect("a decoded ObjectInfo must always re-encode");
    assert_eq!(
        data, re_encoded,
        "ObjectInfo is not byte-identical across decode and re-encode"
    );
});
