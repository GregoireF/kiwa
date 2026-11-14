// SPDX-License-Identifier: MIT OR Apache-2.0
//! Any input: the parser must return, never panic or hang.
#![no_main]
use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| {
    if let Ok(text) = std::str::from_utf8(data) {
        let _ = kiwa_core::detect::parse_make_targets(text);
    }
});
