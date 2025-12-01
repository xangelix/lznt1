//! # LZNT1 Decompression
//!
//! `lznt1` is a safe, pure-Rust implementation of the LZNT1 decompression algorithm.
//! This compression format is commonly used by the Windows NT kernel and in NTFS compression.
//!
//! ## Example
//!
//! ```rust
//! extern crate alloc;
//! use lznt1::decompress;
//! use alloc::vec::Vec;
//!
//! // "Hello world" compressed
//! // Header: Size 13 (0xC + 1)
//! // Tag 1: 8 literals ("Hello wo")
//! // Tag 2: 3 literals ("rld")
//! let compressed_data = [
//!     0x0c, 0xb0,
//!     0x00,       
//!     b'H', b'e', b'l', b'l', b'o', b' ', b'w', b'o',
//!     0x00,
//!     b'r', b'l', b'd',
//! ];
//!
//! let mut buffer = Vec::new();
//! decompress(&compressed_data, &mut buffer).expect("Decompression failed");
//! assert_eq!(buffer, b"Hello world");
//! ```

#![no_std]
#![forbid(unsafe_code)]

extern crate alloc;

pub mod compress;
pub mod decompress;
pub mod error;

pub use compress::compress;
pub use decompress::decompress;
pub use error::DecompressionError;

#[cfg(test)]
mod tests {
    use alloc::vec::Vec;

    use super::{compress, decompress};

    #[test]
    fn test_round_trip() {
        let original = b"Hello world repeated Hello world repeated Hello world repeated";
        let mut compressed = Vec::new();
        let mut decompressed = Vec::new();

        compress(original, &mut compressed);
        decompress(&compressed, &mut decompressed).unwrap();

        assert_eq!(original.to_vec(), decompressed);
    }

    #[test]
    fn test_compress_rle() {
        let original = alloc::vec![b'A'; 100];
        let mut compressed = Vec::new();
        compress(&original, &mut compressed);

        // LZNT1 should compress this heavily using offset=1 tuples
        assert!(compressed.len() < original.len());

        let mut decompressed = Vec::new();
        decompress(&compressed, &mut decompressed).unwrap();
        assert_eq!(original, decompressed);
    }

    #[test]
    fn test_incompressible() {
        // Random data often expands or stays same size (due to fallback)
        // Header overhead is 2 bytes per 4KB.
        let original: Vec<u8> = (0..200).map(|i| (i * 7) as u8).collect();
        let mut compressed = Vec::new();
        compress(&original, &mut compressed);

        // Should use fallback (0x3000 header) + original data
        // Size = 2 (Header) + 200 = 202
        assert_eq!(compressed.len(), 202);

        let mut decompressed = Vec::new();
        decompress(&compressed, &mut decompressed).unwrap();
        assert_eq!(original, decompressed);
    }
}
