#![no_main]
//! Coverage-guided fuzzing of the segment parser.
//!
//! Invariant: `Segment::from_untrusted_bytes` must reject arbitrary/adversarial
//! bytes with an `Err` — never panic, never read out of bounds, never allocate
//! attacker-controlled amounts of memory. (A real, previously-shipped DoS — a
//! 159 GB allocation from an unvalidated length field — was found by fuzzing and
//! fixed; this target guards against regressions.)
//!
//! `cargo +nightly fuzz run segment_parse`

use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| {
    // Result is intentionally ignored: success or a clean error are both fine.
    // The only failure mode that matters is a panic / OOM / OOB, which libFuzzer
    // detects directly.
    let _ = l5m_core::Segment::from_untrusted_bytes(data);
});
