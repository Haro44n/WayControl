#![no_main]

use libfuzzer_sys::fuzz_target;
use waycontrol_protocol::RequestEnvelope;

const MAX_REQUEST_BYTES: usize = 64 * 1024;

fuzz_target!(|data: &[u8]| {
    if data.len() > MAX_REQUEST_BYTES {
        return;
    }

    if let Ok(request) = serde_json::from_slice::<RequestEnvelope>(data) {
        let _ = request.validate();
    }
});
