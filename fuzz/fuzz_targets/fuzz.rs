#![no_main]

use libfuzzer_sys::fuzz_target;
use lznt1::{compress, decompress};

/// Verifies that the decompressor safely handles arbitrary, potentially malformed input.
///
/// This simulates scenarios involving corrupted files, malicious payloads, or random noise.
///
/// # Invariant
/// The decompressor must return either `Ok(_)` or `Err(_)`. It must **never** panic
/// or cause memory safety violations (segfaults), regardless of the input data.
fn verify_decompression_robustness(data: &[u8]) {
    let mut output = Vec::new();
    // We explicitly ignore the result. Whether it succeeds (coincidentally valid)
    // or fails (invalid data) is irrelevant; we only assert that it returns safely.
    let _ = decompress(data, &mut output);
}

/// Verifies the lossless "Round-Trip" property of the compression algorithm.
///
/// # Invariant
/// `decompress(compress(data)) == data`
///
/// If this invariant fails, it implies one of three critical issues:
/// 1. The compressor discarded information.
/// 2. The decompressor corrupted the restored data.
/// 3. The compressor produced output that the decompressor rejects as invalid.
///
/// # Panics
/// This function panics if the decompressed output does not bit-match the input,
/// or if decompression returns an error. These panics signal a fuzzing failure.
fn verify_round_trip(data: &[u8]) {
    let mut compressed = Vec::new();
    compress(data, &mut compressed);

    let mut decompressed = Vec::new();
    match decompress(&compressed, &mut decompressed) {
        Ok(_) => {
            if decompressed != data {
                panic!(
                    "Round-trip mismatch!\nInput len: {}\nCompressed len: {}\nDecompressed len: {}",
                    data.len(),
                    compressed.len(),
                    decompressed.len()
                );
            }
        }
        Err(e) => {
            panic!(
                "Round-trip failed! Decompressor rejected valid compressed data.\nError: {:?}\nInput len: {}",
                e,
                data.len()
            );
        }
    }
}

fuzz_target!(|data: &[u8]| {
    // 1. Robustness: Ensure random noise doesn't crash the decompressor.
    verify_decompression_robustness(data);

    // 2. Correctness: Ensure valid data survives a compress-decompress cycle.
    verify_round_trip(data);
});
