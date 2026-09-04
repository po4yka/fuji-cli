#![no_main]

use fujicli::{ContainerInfo, decode_exact};
use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| {
    // The container header must decode only when every declared field is
    // present, and `payload_len` must then succeed exactly when the wire
    // length covers the header. Anything else is an arithmetic or framing
    // bug in the transaction layer.
    let Ok(info) = decode_exact::<ContainerInfo>(data) else {
        return;
    };
    let covers_header = info.total_len >= ContainerInfo::SIZE as u32;
    assert_eq!(
        info.payload_len().is_ok(),
        covers_header,
        "payload_len must succeed exactly when total_len covers the \
         header (total_len = {})",
        info.total_len,
    );
});
