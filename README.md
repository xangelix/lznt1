# lznt1

[![Crates.io](https://img.shields.io/crates/v/lznt1)](https://crates.io/crates/lznt1)
[![Docs.rs](https://docs.rs/lznt1/badge.svg)](https://docs.rs/lznt1)
[![License](https://img.shields.io/crates/l/lznt1)](https://spdx.org/licenses/MIT)

A safe, pure-Rust, `no_std` implementation of the **LZNT1** compression algorithm.

LZNT1 is the standard compression algorithm used by the Windows NT kernel, notably in **NTFS filesystem compression**, Active Directory replication, and various Windows API functions (`RtlCompressBuffer`). This crate allows you to read and write LZNT1 streams on any platform without linking to system libraries.

## ✨ Features

* **Pure Rust**: No C dependencies or bindings.
* **Safe**: Enforced via `#![forbid(unsafe_code)]`.
* **`no_std` Compatible**: only requires the `alloc` crate.
* **Robust**: Extensively fuzz-tested to ensure resilience against malformed inputs.
* **Simple API**: Straightforward `compress` and `decompress` functions operating on byte slices and vectors.

## 📦 Installation

Add this to your `Cargo.toml`:

```toml
[dependencies]
lznt1 = "0.1.0" # Use the latest version
````

## 🚀 Usage

### Decompression

```rust
use lznt1::decompress;

fn main() -> Result<(), lznt1::DecompressionError> {
    // Example: "Hello world" compressed
    let compressed_data = [
        0x0c, 0xb0, 0x00, 
        b'H', b'e', b'l', b'l', b'o', b' ', b'w', b'o',
        0x00, 
        b'r', b'l', b'd',
    ];

    let mut buffer = Vec::new();
    decompress(&compressed_data, &mut buffer)?;

    assert_eq!(buffer, b"Hello world");
    Ok(())
}
```

### Compression

```rust
use lznt1::compress;

fn main() {
    let input = b"The quick brown fox jumps over the lazy dog. The quick brown fox jumps over the lazy dog.";
    let mut compressed_output = Vec::new();

    compress(input, &mut compressed_output);

    println!("Original size: {}", input.len());
    println!("Compressed size: {}", compressed_output.len());
}
```

## 🛠️ Technical Details

LZNT1 works by splitting data into **4KB chunks**. Each chunk is stored either:

1.  **Compressed**: A header (`0xB000` + size) followed by LZ77 sequences (literals and offset/length tuples).
2.  **Uncompressed**: A header (`0x3000` + size) followed by raw data (used when compression doesn't save space).

This implementation handles the adaptive window splitting logic and the "Tag Group" format (1 tag byte per 8 items) defined by the algorithm.

## 🧪 Testing & Fuzzing

This crate prioritizes correctness and safety.

  * **Unit Tests**: Comprehensive suite covering edge cases, boundary conditions, and RLE patterns.
  * **Fuzzing**: Integration with `cargo-fuzz` to verify that the decompressor never panics or crashes, even on malicious/random input.
  * **Benchmarks**: Performance tracked via `criterion`.

To run the tests:

```bash
cargo test
```

To run benchmarks:

```bash
cargo bench
```

## ⚖️ License

This project is licensed under the [MIT License](https://spdx.org/licenses/MIT).
