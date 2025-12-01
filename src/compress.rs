use alloc::vec::Vec;

/// Standard chunk size for LZNT1 compression (4KB).
const CHUNK_SIZE: usize = 4096;

/// Minimum match length required to encode a compression tuple.
const MIN_MATCH: usize = 3;

/// Absolute hard limit for match length (12 bits + 3).
const MAX_MATCH: usize = 4098;

/// Maximum number of hash chain entries to inspect per position.
/// Limits worst-case performance to O(N * Depth) rather than O(N^2).
const MAX_SEARCH_DEPTH: usize = 16;

/// Hash mask for the 4096-entry table (12 bits).
const HASH_MASK: usize = 0xFFF;

/// Marker for an empty hash table entry.
const EMPTY_ENTRY: u16 = 0xFFFF;

/// Header flags for compressed vs uncompressed chunks.
const HEADER_COMPRESSED: u16 = 0xB000;
const HEADER_RAW: u16 = 0x3000;

/// Internal helper struct to manage the LZNT1 "Tag Group" logic.
///
/// A Tag Group consists of 1 flag byte followed by up to 8 tokens (literals or tuples).
/// The flag byte contains one bit per token (0=Literal, 1=Tuple).
struct TagAccumulator {
    tag_byte: u8,
    item_count: usize,
    buffer: [u8; 16], // Max size: 8 tuples * 2 bytes = 16 bytes
    buffer_len: usize,
}

impl TagAccumulator {
    const fn new() -> Self {
        Self {
            tag_byte: 0,
            item_count: 0,
            buffer: [0; 16],
            buffer_len: 0,
        }
    }

    /// Adds a literal byte to the current group.
    fn push_literal(&mut self, byte: u8, output: &mut Vec<u8>) {
        // Tag bit 0 is implicit (do nothing to tag_byte)
        self.buffer[self.buffer_len] = byte;
        self.buffer_len += 1;
        self.commit_item(output);
    }

    /// Adds a compressed tuple (offset/length pair) to the current group.
    fn push_tuple(&mut self, tuple: u16, output: &mut Vec<u8>) {
        // Set tag bit to 1 at the current item index
        self.tag_byte |= 1 << self.item_count;

        // Write 2-byte tuple (Little Endian)
        let bytes = tuple.to_le_bytes();
        self.buffer[self.buffer_len] = bytes[0];
        self.buffer[self.buffer_len + 1] = bytes[1];
        self.buffer_len += 2;

        self.commit_item(output);
    }

    /// Increments the item count and flushes the group if full (8 items).
    fn commit_item(&mut self, output: &mut Vec<u8>) {
        self.item_count += 1;
        if self.item_count == 8 {
            self.flush(output);
        }
    }

    /// Writes the current tag group to the output vector and resets state.
    fn flush(&mut self, output: &mut Vec<u8>) {
        if self.item_count > 0 {
            output.push(self.tag_byte);
            output.extend_from_slice(&self.buffer[..self.buffer_len]);
            // Reset
            self.tag_byte = 0;
            self.item_count = 0;
            self.buffer_len = 0;
        }
    }
}

/// Context to hold reusable memory for compression to avoid allocation churn.
pub struct Lznt1Context {
    // Maps a 3-byte hash to the *most recent* position in the chunk.
    head: [u16; CHUNK_SIZE],
    // Maps a position to the *previous* position with the same hash.
    next: [u16; CHUNK_SIZE],
}

impl Default for Lznt1Context {
    fn default() -> Self {
        Self::new()
    }
}

impl Lznt1Context {
    #[must_use]
    pub const fn new() -> Self {
        Self {
            head: [EMPTY_ENTRY; CHUNK_SIZE],
            next: [EMPTY_ENTRY; CHUNK_SIZE],
        }
    }

    /// Resets the hash table for a new chunk.
    fn reset(&mut self) {
        self.head.fill(EMPTY_ENTRY);
    }

    /// Updates the hash chain for the given index.
    ///
    /// This should be called for every byte processed (literal or matched) to allow
    /// overlapping matches in future searches.
    fn update(&mut self, input: &[u8], idx: usize) {
        if idx + MIN_MATCH <= input.len() {
            let h = hash_3_bytes(&input[idx..idx + 3]);
            // Safe because idx < CHUNK_SIZE during compression
            self.next[idx] = self.head[h];
            self.head[h] = idx as u16;
        }
    }
}

/// Compresses the entire input into the output vector using the LZNT1 algorithm.
///
/// This function processes the input in 4KB chunks. For each chunk, it decides
/// whether to store it compressed or raw (uncompressed) based on which is smaller.
///
/// # Parameters
/// * `input`: The source data to compress.
/// * `output`: The destination vector (appended to).
pub fn compress(input: &[u8], output: &mut Vec<u8>) {
    let mut ctx = Lznt1Context::new();
    let mut src_pos = 0;

    while src_pos < input.len() {
        let chunk_len = (input.len() - src_pos).min(CHUNK_SIZE);
        let chunk = &input[src_pos..src_pos + chunk_len];

        let start_out = output.len();
        // Reserve space for Header (2 bytes)
        output.extend_from_slice(&[0, 0]);

        compress_chunk(chunk, output, &mut ctx);

        let compressed_len = output.len() - start_out - 2;

        if compressed_len < chunk.len() {
            // Success: Overwrite header with Compressed flag + size
            let header = encode_header(HEADER_COMPRESSED, compressed_len);
            let h_bytes = header.to_le_bytes();
            output[start_out] = h_bytes[0];
            output[start_out + 1] = h_bytes[1];
        } else {
            // Failure: Expansion or no savings. Revert and store Raw.
            output.truncate(start_out);
            let header = encode_header(HEADER_RAW, chunk.len());
            output.extend_from_slice(&header.to_le_bytes());
            output.extend_from_slice(chunk);
        }

        src_pos += chunk_len;
    }
}

/// Compresses a single chunk (max 4096 bytes).
fn compress_chunk(chunk: &[u8], output: &mut Vec<u8>, ctx: &mut Lznt1Context) {
    ctx.reset();
    let mut accumulator = TagAccumulator::new();

    // Adaptive State
    let mut blob_out_len = 0; // "Uncompressed" bytes represented so far
    let mut split = 12; // 12 bits Length, 4 bits Offset
    let mut threshold = 16; // When blob_out_len > threshold, shift parameters

    let mut in_idx = 0;

    while in_idx < chunk.len() {
        // Current max bits allowed for offset based on adaptive split
        let off_bits = 16 - split;
        let max_offset = 1 << off_bits;

        let mut best_len = 0;
        let mut best_off = 0;

        // --- 1. Find Best Match ---
        if in_idx + MIN_MATCH <= chunk.len() {
            let hash = hash_3_bytes(&chunk[in_idx..in_idx + 3]);
            let mut candidate_idx = ctx.head[hash];
            let mut depth = 0;

            while candidate_idx != EMPTY_ENTRY && depth < MAX_SEARCH_DEPTH {
                let candidate = candidate_idx as usize;

                if candidate >= in_idx {
                    break; // Should not happen with correct logic
                }

                let dist = in_idx - candidate;
                if dist >= max_offset {
                    break; // Too far for current adaptive window
                }

                // Optimization: Check the byte at `best_len` to fail fast
                if in_idx + best_len < chunk.len()
                    && chunk[candidate + best_len] == chunk[in_idx + best_len]
                {
                    let match_len =
                        common_prefix_len(&chunk[in_idx..], &chunk[candidate..], MAX_MATCH);

                    if match_len >= MIN_MATCH && match_len > best_len {
                        best_len = match_len;
                        best_off = dist;
                        if best_len >= MAX_MATCH {
                            best_len = MAX_MATCH;
                            break;
                        }
                    }
                }

                candidate_idx = ctx.next[candidate];
                depth += 1;
            }
        }

        // --- 2. Encode Match or Literal ---
        if best_len >= MIN_MATCH {
            // Clamp length to fit in current `split` bits
            // Max encodable length = (2^split) + 3 - 1
            let max_len_encodable = (1 << split) + 2;
            if best_len > max_len_encodable {
                best_len = max_len_encodable;
            }

            // Tuple = ((off - 1) << split) | (len - 3)
            let len_val = best_len - 3;
            let off_val = best_off - 1;
            let tuple = ((off_val << split) | len_val) as u16;

            accumulator.push_tuple(tuple, output);

            // Update hash for all bytes covered by the match
            for _ in 0..best_len {
                ctx.update(chunk, in_idx);
                in_idx += 1;
            }
            blob_out_len += best_len;
        } else {
            // Literal
            accumulator.push_literal(chunk[in_idx], output);
            ctx.update(chunk, in_idx);
            in_idx += 1;
            blob_out_len += 1;
        }

        // --- 3. Adaptive Threshold Update ---
        while blob_out_len > threshold {
            if split > 0 {
                split -= 1;
            }
            threshold <<= 1;
        }
    }

    // Flush any remaining items in the accumulator
    accumulator.flush(output);
}

/// Helper to format the 2-byte chunk header.
/// Header format: `Flag | (Size - 1) & 0xFFF`
#[inline]
const fn encode_header(flag: u16, size: usize) -> u16 {
    flag | ((size - 1) as u16 & 0x0FFF)
}

/// Hashes the first 3 bytes of a slice for the LZNT1 dictionary lookup.
#[inline]
fn hash_3_bytes(b: &[u8]) -> usize {
    let h = ((b[0] as usize) << 6) ^ ((b[1] as usize) << 3) ^ (b[2] as usize);
    h & HASH_MASK
}

/// Finds the length of the common prefix between two slices, up to `max`.
#[inline]
fn common_prefix_len(a: &[u8], b: &[u8], max: usize) -> usize {
    let limit = a.len().min(b.len()).min(max);
    let mut len = 0;
    while len < limit && a[len] == b[len] {
        len += 1;
    }
    len
}
