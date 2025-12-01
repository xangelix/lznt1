use crate::error::DecompressionError;
use alloc::vec::Vec;

type Result<T> = core::result::Result<T, DecompressionError>;

// --- Constants ---

/// Bitmask to extract the chunk size (lower 12 bits) from the header.
const HEADER_SIZE_MASK: u16 = 0x0FFF;

/// Bit flag indicating if the chunk is compressed (0xBxxx) or raw (0x3xxx).
/// LZNT1 typically uses the MSB or specific high nibbles, but checking 0x8000 is sufficient.
const HEADER_COMPRESSED_FLAG: u16 = 0x8000;

/// Number of items (literals or tuples) in a single tag group.
const TAG_GROUP_SIZE: usize = 8;

/// Initial bit width for the length component of a match tuple.
const INITIAL_SPLIT: usize = 12;

/// Initial threshold for the uncompressed size before adaptive state update.
const INITIAL_THRESHOLD: usize = 16;

/// Decompresses an entire LZNT1 stream.
///
/// The input is processed in chunks (headers + data). The function manages
/// output capacity reservation and validates the integrity of chunk headers.
pub fn decompress(input: &[u8], output: &mut Vec<u8>) -> Result<()> {
    // Heuristic capacity reservation to reduce allocation churn.
    let heuristic_cap = input.len();
    if output.capacity() < output.len() + heuristic_cap {
        output.reserve(heuristic_cap);
    }

    let mut in_pos = 0;
    let end = input.len();

    while in_pos < end {
        // LZNT1 streams may be null-terminated (single 0x00 byte at EOF).
        if in_pos + 1 == end && input[in_pos] == 0 {
            break;
        }

        // Ensure we can read the 2-byte header.
        if in_pos + 2 > end {
            return Err(DecompressionError::UnexpectedEof);
        }

        let header = u16::from_le_bytes([input[in_pos], input[in_pos + 1]]);
        in_pos += 2;

        if header == 0 {
            break; // Standard End-of-Stream marker
        }

        let size = ((header & HEADER_SIZE_MASK) + 1) as usize;
        let is_compressed = (header & HEADER_COMPRESSED_FLAG) != 0;

        // Ensure the chunk body is within bounds.
        if in_pos + size > end {
            return Err(DecompressionError::InputTooShort);
        }

        let block_slice = &input[in_pos..in_pos + size];

        if is_compressed {
            decompress_compressed_block(block_slice, output)?;
        } else {
            // Raw block: direct copy
            output.extend_from_slice(block_slice);
        }

        in_pos += size;
    }

    Ok(())
}

/// Decompresses a single compressed LZNT1 block.
///
/// Handles the "Tag Group" logic, adaptive window splitting, and LZ matches.
fn decompress_compressed_block(input: &[u8], output: &mut Vec<u8>) -> Result<()> {
    let mut in_idx = 0;
    let end = input.len();

    // Adaptive State
    let mut split = INITIAL_SPLIT;
    let mut mask = (1 << split) - 1;
    let mut threshold = INITIAL_THRESHOLD;
    let start_out_len = output.len();

    while in_idx < end {
        // 1. Load Tag Byte
        let tag_byte = input[in_idx];
        in_idx += 1;

        // --- All-Literals Fast Path ---
        // If tag is 0, the next 8 items are literals.
        // We only take this path if we have enough bytes remaining to avoid EOF checks.
        if tag_byte == 0 && in_idx + TAG_GROUP_SIZE <= end {
            output.extend_from_slice(&input[in_idx..in_idx + TAG_GROUP_SIZE]);
            in_idx += TAG_GROUP_SIZE;

            // Update adaptive parameters for the 8 bytes just added.
            update_adaptive_state(
                output.len() - start_out_len,
                &mut threshold,
                &mut split,
                &mut mask,
            );
            continue;
        }

        // 2. Mixed Literals/Links Loop
        for i in 0..TAG_GROUP_SIZE {
            let is_link = (tag_byte >> i) & 1 != 0;

            if is_link {
                // Ensure we have 2 bytes for the tuple.
                if in_idx + 2 > end {
                    return Err(DecompressionError::UnexpectedEof);
                }

                let tuple = u16::from_le_bytes([input[in_idx], input[in_idx + 1]]) as usize;
                in_idx += 2;

                // Decode Length/Offset using current adaptive split
                let length = (tuple & mask) + 3;
                let offset = (tuple >> split) + 1;

                apply_match(output, length, offset)?;
            } else {
                // Literal
                if in_idx >= end {
                    // Valid end of stream inside a literal tag group.
                    // This is a permissive behavior required by LZNT1 specs.
                    return Ok(());
                }
                output.push(input[in_idx]);
                in_idx += 1;
            }

            // Update adaptive parameters after *every* item
            update_adaptive_state(
                output.len() - start_out_len,
                &mut threshold,
                &mut split,
                &mut mask,
            );

            // Check EOF after processing item
            if in_idx >= end {
                return Ok(());
            }
        }
    }

    Ok(())
}

/// Applies an LZ77 match to the output buffer.
///
/// Handles data copying from the existing output history. Includes an optimization
/// for Run-Length Encoding (RLE) where offset is 1.
#[inline]
fn apply_match(output: &mut Vec<u8>, length: usize, offset: usize) -> Result<()> {
    if offset > output.len() {
        return Err(DecompressionError::InvalidOffset);
    }

    output.reserve(length);

    // --- RLE Fast Path (Offset == 1) ---
    // Since offset > 0 (checked implicitly by offset > output.len() if output is empty),
    // and we know output.len() >= offset, output is not empty here.
    if offset == 1 {
        let last_byte = output[output.len() - 1];
        output.resize(output.len() + length, last_byte);
    } else {
        // Standard LZ77 Copy (supports overlapping ranges)
        let src_pos = output.len() - offset;
        for k in 0..length {
            let val = output[src_pos + k];
            output.push(val);
        }
    }

    Ok(())
}

/// Updates the adaptive window parameters (split, mask, threshold) based on
/// the current uncompressed block size.
#[inline]
const fn update_adaptive_state(
    current_block_out_len: usize,
    threshold: &mut usize,
    split: &mut usize,
    mask: &mut usize,
) {
    while current_block_out_len > *threshold {
        if *split > 0 {
            *split -= 1;
            *mask = (1 << *split) - 1;
        }
        *threshold <<= 1;
    }
}
