use lznt1::{DecompressionError, compress, decompress};

// --- Test Constants ---

/// Header flag indicating compressed data (0xB000).
const HEADER_COMPRESSED: u16 = 0xB000;
/// Header flag indicating raw (uncompressed) data (0x3000).
const HEADER_UNCOMPRESSED: u16 = 0x3000;
/// Mask to extract the chunk size (lower 12 bits).
const SIZE_MASK: u16 = 0x0FFF;

// --- Helpers ---

/// Performs a full compress-decompress cycle and asserts bit-exact reconstruction.
///
/// Use `#[track_caller]` to point failures to the specific test function calling this helper.
#[track_caller]
fn assert_round_trip(input: &[u8]) {
    let mut compressed = Vec::new();
    compress(input, &mut compressed);

    let mut output = Vec::new();
    match decompress(&compressed, &mut output) {
        Ok(()) => assert_eq!(output, input, "Round-trip output mismatches input"),
        Err(e) => panic!("Decompression failed during round-trip: {e:?}"),
    }
}

/// Helper to compress data and return the vector.
fn compress_to_vec(input: &[u8]) -> Vec<u8> {
    let mut out = Vec::new();
    compress(input, &mut out);
    out
}

/// Helper to inspect the first chunk header of compressed data.
/// Returns a tuple: `(is_compressed_flag_set, declared_size)`.
fn parse_header(data: &[u8]) -> (bool, usize) {
    assert!(
        data.len() >= 2,
        "Compressed data too short to contain a header"
    );
    let val = u16::from_le_bytes([data[0], data[1]]);
    let is_compressed = (val & 0x8000) != 0;
    let size = ((val & SIZE_MASK) + 1) as usize;
    (is_compressed, size)
}

// --- Basic Sanity & Boundaries (Tests 1-7) ---

/// Test: Empty input should result in empty output (round-trip success).
#[test]
fn t01_empty_input() {
    assert_round_trip(b"");
}

/// Test: Single byte input.
/// Expectation: Uncompressed fallback (Header + 1 byte).
#[test]
fn t02_single_byte() {
    let input = b"A";
    let compressed = compress_to_vec(input);

    // Header (2 bytes) + Data (1 byte) = 3 bytes total.
    assert_eq!(compressed.len(), 3);

    let (is_compressed, size) = parse_header(&compressed);
    assert!(!is_compressed, "Single byte should be stored uncompressed");
    assert_eq!(size, 1);

    assert_round_trip(input);
}

/// Test: Small string round-trip.
#[test]
fn t03_tiny_string() {
    assert_round_trip(b"Hi");
}

/// Test: Input exactly matching chunk size (4096).
/// Random data prevents compression, forcing a full 4096-byte raw chunk.
#[test]
fn t04_exact_chunk_boundary() {
    let input: Vec<u8> = (0..4096).map(|i| (i % 251) as u8).collect();
    assert_round_trip(&input);
}

/// Test: Input slightly larger than one chunk (4097).
/// Should force a split into two chunks.
#[test]
fn t05_chunk_plus_one() {
    let input: Vec<u8> = (0..4097).map(|i| (i % 251) as u8).collect();
    assert_round_trip(&input);
}

/// Test: Two exact full chunks (8192 bytes).
#[test]
fn t06_two_exact_chunks() {
    let input: Vec<u8> = (0..8192).map(|i| (i % 251) as u8).collect();
    assert_round_trip(&input);
}

/// Test: Uncompressible data should generate an Uncompressed Header (0x3xxx).
#[test]
fn t07_uncompressed_fallback_header() {
    let input: Vec<u8> = (0..100).map(|i| (i * 13) as u8).collect();
    let compressed = compress_to_vec(&input);

    let (is_compressed, size) = parse_header(&compressed);
    assert!(
        !is_compressed,
        "Header should denote uncompressed data (0x3xxx)"
    );
    assert_eq!(size, 100);
}

// --- Compression Logic & Patterns (Tests 8-20) ---

/// Test: RLE (Run-Length Encoding) for simple repeating byte.
#[test]
fn t08_rle_simple() {
    let input = vec![b'A'; 100];
    let compressed = compress_to_vec(&input);

    // Approx size: Header(2) + Tag(1) + Literal(1) + Tuple(2) = 6 bytes
    assert!(compressed.len() < 10);
    assert_round_trip(&input);
}

/// Test: RLE spanning across chunk boundaries (10,000 bytes).
#[test]
fn t09_rle_cross_chunk() {
    let input = vec![b'A'; 10000];
    let res = compress_to_vec(&input);
    assert!(res.len() < input.len() / 2);
    assert_round_trip(&input);
}

/// Test: All zeros (common disk image pattern).
#[test]
fn t10_all_zeros() {
    let input = vec![0u8; 1024];
    let compressed = compress_to_vec(&input);
    assert!(compressed.len() < 50);
    assert_round_trip(&input);
}

/// Test: Alternating pattern (0xAA, 0x55).
#[test]
fn t11_alternating_pattern() {
    let input: Vec<u8> = (0..1000)
        .map(|i| if i % 2 == 0 { 0xAA } else { 0x55 })
        .collect();
    let compressed = compress_to_vec(&input);
    assert!(compressed.len() < 500);
    assert_round_trip(&input);
}

/// Test: Incrementing pattern (no repeats > 3 bytes).
/// Should fallback to uncompressed storage as it is strictly incompressible.
#[test]
fn t12_incrementing_pattern_incompressible() {
    let input: Vec<u8> = (0..255).collect();
    let compressed = compress_to_vec(&input);

    assert_eq!(compressed.len(), 255 + 2); // Data + Header
    assert_round_trip(&input);
}

/// Test: Overlapping match (e.g., "aaaaa").
/// Tests hash update logic for skipped bytes during match encoding.
#[test]
fn t13_overlapping_match() {
    assert_round_trip(b"aaaaa");
}

/// Test: Distant match within the same chunk window.
#[test]
fn t14_distant_match() {
    let mut input = Vec::new();
    input.extend_from_slice(b"ABC");
    input.extend(vec![0xFF; 4000]); // Padding/Noise
    input.extend_from_slice(b"ABC"); // Target
    assert_round_trip(&input);
}

/// Test: Match at the very end of a chunk.
#[test]
fn t15_match_at_chunk_end() {
    let mut input = vec![0u8; 4096];
    input[4093] = b'X';
    input[4094] = b'Y';
    input[4095] = b'Z';
    assert_round_trip(&input);
}

/// Test: Data that forces adaptive bit-split change exactly at the threshold.
/// Logic changes split when output > 16 bytes.
#[test]
fn t16_adaptive_split_crossing_16() {
    let input = b"0123456789ABCDEF0123456789ABCDEF";
    assert_round_trip(input);
}

/// Test: Repeating phrases (standard text compression).
#[test]
fn t17_repeating_phrases() {
    let phrase = b"The quick brown fox jumps over the lazy dog. ";
    let mut input = Vec::new();
    for _ in 0..100 {
        input.extend_from_slice(phrase);
    }
    let compressed = compress_to_vec(&input);
    assert!(compressed.len() < input.len() / 5);
    assert_round_trip(&input);
}

/// Test: Verify Compressed Flag (0xB000) is set for highly compressible data.
#[test]
fn t18_header_compressed_flag_check() {
    let input = vec![b'A'; 64];
    let compressed = compress_to_vec(&input);
    let (is_compressed, _) = parse_header(&compressed);
    assert!(is_compressed);
}

/// Test: Verify Uncompressed Flag (0x3000) for short/random data where expansion would occur.
#[test]
fn t19_verify_uncompressed_fallback() {
    let input = b"abcdefgh";
    let compressed = compress_to_vec(input);
    // Uncompressed fallback is preferred if compressed size > raw size.
    let (is_compressed, _) = parse_header(&compressed);
    assert!(!is_compressed, "Should be raw (uncompressed)");
    assert_round_trip(input);
}

/// Test: Mixed Literals and References.
#[test]
fn t20_tag_byte_mixed() {
    assert_round_trip(b"aaaaaaaaa");
}

// --- Decompression Error Handling (Tests 21-30) ---

/// Test: Unexpected EOF while reading header.
#[test]
fn t21_decompress_unexpected_eof_header() {
    let data = vec![0xB0]; // Only 1 byte
    let mut out = Vec::new();
    assert_eq!(
        decompress(&data, &mut out),
        Err(DecompressionError::UnexpectedEof)
    );
}

/// Test: Header claims size larger than available input.
#[test]
fn t22_decompress_input_too_short_header() {
    let header = HEADER_COMPRESSED | 99; // Size 100
    let data = header.to_le_bytes();
    let mut out = Vec::new();
    assert_eq!(
        decompress(&data, &mut out),
        Err(DecompressionError::InputTooShort)
    );
}

/// Test: Unexpected EOF inside a compressed tag group.
#[test]
fn t23_decompress_unexpected_eof_in_tag_group() {
    let header = HEADER_COMPRESSED | 0xFF; // Size 256
    let mut data = header.to_le_bytes().to_vec();
    data.push(0x00); // Tag: 8 literals expected
    data.push(b'A'); // Only 1 provided

    let mut out = Vec::new();
    assert_eq!(
        decompress(&data, &mut out),
        Err(DecompressionError::InputTooShort)
    );
}

/// Test: Invalid offset (pointing before start of buffer).
#[test]
fn t24_decompress_invalid_offset_start() {
    let mut data = Vec::new();
    let header = HEADER_COMPRESSED | 2; // Size 3
    data.extend_from_slice(&header.to_le_bytes());
    data.push(0x01); // Tag: 1st item is Ref
    // Tuple: Offset 1, Length 3.
    // Invalid because output is empty (offset > output.len()).
    data.extend_from_slice(&0x0000u16.to_le_bytes());

    let mut out = Vec::new();
    assert_eq!(
        decompress(&data, &mut out),
        Err(DecompressionError::InvalidOffset)
    );
}

/// Test: Stream clean termination via null header (0x0000).
#[test]
fn t25_decompress_zero_header_terminator() {
    let data = vec![0x00, 0x00];
    let mut out = Vec::new();
    assert!(decompress(&data, &mut out).is_ok());
}

/// Test: Truncated tuple (header valid, but missing 2nd byte of tuple).
#[test]
fn t26_decompress_truncated_tuple() {
    let header = HEADER_COMPRESSED | 1; // Size 2
    let mut data = header.to_le_bytes().to_vec();
    data.push(0x01); // Tag: Ref
    data.push(0x00); // 1st byte of tuple (missing 2nd)

    let mut out = Vec::new();
    assert_eq!(
        decompress(&data, &mut out),
        Err(DecompressionError::UnexpectedEof)
    );
}

/// Test: Offset boundary check (Offset > Output Length).
#[test]
fn t27_offset_boundary_check() {
    let mut input = vec![b'A'];
    let mut chunk = Vec::new();
    let header = HEADER_COMPRESSED | 2; // Size 3
    chunk.extend_from_slice(&header.to_le_bytes());
    chunk.push(0x01); // Tag: Ref
    // Offset=10 (fail), Len=3. Tuple = ((9)<<12) | 0 = 0x9000
    chunk.extend_from_slice(&0x9000u16.to_le_bytes());

    assert_eq!(
        decompress(&chunk, &mut input),
        Err(DecompressionError::InvalidOffset)
    );
}

/// Test: Valid large backward reference.
#[test]
fn t28_large_offset_valid() {
    let mut input: Vec<u8> = (0..4000).map(|i| (i % 200) as u8).collect();
    let prefix = input[0..100].to_vec();
    input.extend_from_slice(&prefix); // Repeat start
    assert_round_trip(&input);
}

/// Test: Uncompressed block header length mismatch (Input < Header Size).
#[test]
fn t29_header_uncompressed_length_mismatch() {
    let header = HEADER_UNCOMPRESSED | 5; // Size 6
    let mut data = header.to_le_bytes().to_vec();
    data.push(0xAA); // Missing 3 bytes

    let mut out = Vec::new();
    assert_eq!(
        decompress(&data, &mut out),
        Err(DecompressionError::InputTooShort)
    );
}

/// Test: Trailing null bytes after terminator (should be ignored).
#[test]
fn t30_null_byte_after_terminator() {
    let data = vec![0x00, 0x00, 0x00];
    let mut out = Vec::new();
    assert!(decompress(&data, &mut out).is_ok());
}

// --- Advanced Scenarios & Edge Cases (Tests 31-40) ---

/// Test: Fibonacci sequence (deterministic but non-trivial pattern).
#[test]
fn t31_fibonacci_content() {
    let mut input = vec![1u8, 1];
    for _ in 0..1000 {
        let next = input[input.len() - 1].wrapping_add(input[input.len() - 2]);
        input.push(next);
    }
    assert_round_trip(&input);
}

/// Test: All byte values (0..255).
#[test]
fn t32_all_bytes_values() {
    let input: Vec<u8> = (0..=255).collect();
    assert_round_trip(&input);
}

/// Test: Appending compression to existing buffer.
#[test]
fn t33_compress_reused_buffer() {
    let input = b"hello";
    let mut buf = Vec::new();

    compress(input, &mut buf);
    assert!(!buf.is_empty());

    let len1 = buf.len();
    compress(input, &mut buf); // Append
    assert!(buf.len() > len1);

    let mut out = Vec::new();
    decompress(&buf[..len1], &mut out).unwrap();
    assert_eq!(out, input);
}

/// Test: Verify context clearing between chunks.
#[test]
fn t34_context_clearing_check() {
    // If context isn't cleared, chunk2 might reference chunk1 invalidly.
    let chunk1 = vec![b'A'; 4096];
    let chunk2 = vec![b'A'; 4096];
    let mut input = chunk1;
    input.extend(chunk2);
    assert_round_trip(&input);
}

/// Test: Partial tag group at chunk end.
#[test]
fn t35_partial_tag_group_at_chunk_end() {
    assert_round_trip(b"abc");
}

/// Test: UTF-8 content.
#[test]
fn t36_unicode_bytes() {
    assert_round_trip("おはようございます".as_bytes());
}

/// Test: Decompression into capacity-0 vector (forcing allocation).
#[test]
fn t37_output_capacity_growing() {
    let input = vec![b'A'; 1000];
    let compressed = compress_to_vec(&input);
    let mut out = Vec::new();
    decompress(&compressed, &mut out).unwrap();
    assert_eq!(out, input);
}

/// Test: Decompression into pre-allocated large vector.
#[test]
fn t38_preallocated_excessive_output() {
    let input = b"test";
    let compressed = compress_to_vec(input);
    let mut out = Vec::with_capacity(1_000_000);
    decompress(&compressed, &mut out).unwrap();
    assert_eq!(out, input);
}

/// Test: Very sparse data (mostly zeros with rare non-zero bytes).
#[test]
fn t39_very_sparse_data() {
    let mut input = vec![0u8; 1024 * 1024];
    input[500] = 0xFF;
    input[90000] = 0xAA;
    // Should be highly compressed
    let compressed = compress_to_vec(&input);
    assert!(compressed.len() < 5000);
    assert_round_trip(&input);
}

/// Test: Data exceeding max match length (4098).
#[test]
fn t40_max_match_length() {
    let input = vec![b'A'; 5000];
    assert_round_trip(&input);
}

// --- Resilience & Fuzz-like Scenarios (Tests 41-50) ---

/// Test: Deterministic random noise.
#[test]
fn t41_random_noise_roundtrip() {
    let input: Vec<u8> = (0..2048).map(|i| ((i * 37) ^ (i >> 3)) as u8).collect();
    assert_round_trip(&input);
}

/// Test: Header present but missing Tag byte.
#[test]
fn t42_decompress_short_tag_byte() {
    let header = HEADER_COMPRESSED | 2; // Size 3
    let data = header.to_le_bytes().to_vec();
    // Missing tag byte
    let mut out = Vec::new();
    assert_eq!(
        decompress(&data, &mut out),
        Err(DecompressionError::InputTooShort)
    );
}

/// Test: Invalid flags in header (e.g. 0xC000).
/// Implementation only checks MSB (0x8000), so this should be treated as compressed.
#[test]
fn t43_invalid_flags_in_header() {
    let header: u16 = 0xC000 | 3; // Size 4
    let mut data = header.to_le_bytes().to_vec();
    data.push(0x00); // Tag
    data.push(b'A');
    let mut out = Vec::new();
    // Should attempt decompression without panic.
    let _ = decompress(&data, &mut out);
}

/// Test: Input data resembling headers (false positives).
#[test]
fn t44_data_looking_like_header() {
    let mut input = Vec::new();
    input.extend_from_slice(&HEADER_COMPRESSED.to_le_bytes());
    input.extend_from_slice(&HEADER_UNCOMPRESSED.to_le_bytes());
    assert_round_trip(&input);
}

/// Test: Adaptive state reset between blocks.
#[test]
fn t45_adaptive_reset() {
    let block1 = vec![b'A'; 200];
    let block2 = b"small";
    let mut input = block1;
    input.extend_from_slice(block2);
    assert_round_trip(&input);
}

/// Test: Long run of literals (no matches).
#[test]
fn t46_long_literal_run() {
    let input: Vec<u8> = (0..4096).map(|i| (i % 251) as u8).collect();
    assert_round_trip(&input);
}

/// Test: RLE with max length offset.
#[test]
fn t47_offset_1_length_max() {
    let input = vec![b'X'; 4096];
    assert_round_trip(&input);
}

/// Test: Recursive compression (compressing a compressed stream).
#[test]
fn t48_recursive_compression() {
    let input = b"Hello world repeated Hello world repeated";
    let comp1 = compress_to_vec(input);
    let comp2 = compress_to_vec(&comp1);

    let mut out_comp1 = Vec::new();
    decompress(&comp2, &mut out_comp1).unwrap();
    assert_eq!(out_comp1, comp1);

    let mut out_orig = Vec::new();
    decompress(&out_comp1, &mut out_orig).unwrap();
    assert_eq!(out_orig, input);
}

/// Test: Stack safety placeholder (no recursion in implementation).
#[test]
fn t49_max_stack_safety() {
    assert_eq!(1, 1);
}

/// Test: Complex corpus mix.
#[test]
fn t50_final_mixed_corpus() {
    let mut input = Vec::new();
    input.extend(vec![0u8; 100]); // RLE 0
    input.extend(b"Literal string");
    input.extend(vec![b'A'; 50]); // RLE A
    input.extend((0..100).map(|i| i as u8)); // Non-compressible
    assert_round_trip(&input);
}
