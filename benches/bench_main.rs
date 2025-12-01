use criterion::{Criterion, Throughput, criterion_group, criterion_main};
use lznt1::{compress, decompress};
use std::hint::black_box;

/// Generates a vector of pseudo-random bytes using a deterministic Linear Congruential Generator (LCG).
///
/// This ensures benchmarks are reproducible across runs. The generated data has high entropy,
/// representing a "worst-case" scenario for compression algorithms.
///
/// # Parameters
/// * `size` - The number of bytes to generate.
///
/// # Returns
/// A `Vec<u8>` containing the generated random data.
fn generate_random(size: usize) -> Vec<u8> {
    let mut vec = Vec::with_capacity(size);
    // Fixed seed for determinism (0xDEAD_BEEF).
    let mut seed: u64 = 0xDEAD_BEEF;
    for _ in 0..size {
        // Simple LCG: seed = (a * seed + c) % m
        seed = (seed.wrapping_mul(1_664_525).wrapping_add(1_013_904_223)) & 0xFFFF_FFFF;
        vec.push((seed >> 24) as u8);
    }
    vec
}

/// Generates a vector containing repeated standard text sentences.
///
/// The pattern is "The quick brown fox jumps over the lazy dog. ".
/// This represents "typical" compressible data (text logs, JSON, etc.).
///
/// # Parameters
/// * `size` - The target size in bytes.
///
/// # Returns
/// A `Vec<u8>` filled with the repeated text pattern, truncated to `size`.
fn generate_text(size: usize) -> Vec<u8> {
    let text = b"The quick brown fox jumps over the lazy dog. ";
    let mut vec = Vec::with_capacity(size);
    while vec.len() < size {
        vec.extend_from_slice(text);
    }
    vec.truncate(size);
    vec
}

/// Generates a vector filled with zeroes.
///
/// This represents a "best-case" scenario for most compression algorithms (highly repetitive),
/// often handled efficiently by Run-Length Encoding (RLE).
///
/// # Parameters
/// * `size` - The number of bytes to allocate.
///
/// # Returns
/// A `Vec<u8>` initialized to zero.
fn generate_zeroes(size: usize) -> Vec<u8> {
    vec![0u8; size]
}

/// Benchmarks the LZNT1 compression algorithm against various data patterns.
///
/// Scenarios:
/// 1. **Zeroes**: High repetition, RLE-friendly.
/// 2. **Random**: High entropy, generally incompressible.
/// 3. **Text**: Moderate entropy, representative of real-world text.
fn bench_compression(c: &mut Criterion) {
    let mut group = c.benchmark_group("LZNT1 Compression");

    // Bench against a reasonable 64KB block size, typical for chunk-based operations.
    let size = 64 * 1024;

    let scenarios = [
        ("Zeroes", generate_zeroes(size)),
        ("Random", generate_random(size)),
        ("Text", generate_text(size)),
    ];

    for (name, input_data) in &scenarios {
        let bench_name = format!("{name} 64KB");

        group.throughput(Throughput::Bytes(size as u64));
        group.bench_function(&bench_name, |b| {
            // Pre-allocate output to avoid measuring allocation overhead during the loop.
            let mut output = Vec::with_capacity(size);
            b.iter(|| {
                output.clear();
                compress(black_box(input_data), black_box(&mut output));
            });
        });
    }

    group.finish();
}

/// Benchmarks the LZNT1 decompression algorithm.
///
/// Requires pre-compressing the source data before measuring decompression throughput.
/// Throughput is calculated based on the *uncompressed* size to represent the rate
/// of data restoration.
fn bench_decompression(c: &mut Criterion) {
    let mut group = c.benchmark_group("LZNT1 Decompression");
    let size = 64 * 1024;

    let scenarios = [
        ("Zeroes", generate_zeroes(size)),
        ("Random", generate_random(size)),
        ("Text", generate_text(size)),
    ];

    for (name, source_data) in &scenarios {
        // Setup: Compress the data first so we have a valid source for decompression.
        let mut compressed_data = Vec::new();
        compress(source_data, &mut compressed_data);

        let bench_name = format!("{name} 64KB");

        // Throughput metrics are based on the original uncompressed size.
        group.throughput(Throughput::Bytes(size as u64));
        group.bench_function(&bench_name, |b| {
            let mut output = Vec::with_capacity(size);
            b.iter(|| {
                output.clear();
                // We unwrap here to ensure correctness; if decompression fails, the benchmark should fail.
                // This matches the panic behavior of the original file.
                decompress(black_box(&compressed_data), black_box(&mut output)).unwrap();
            });
        });
    }

    group.finish();
}

criterion_group!(benches, bench_compression, bench_decompression);
criterion_main!(benches);
