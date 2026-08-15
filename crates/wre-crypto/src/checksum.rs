use serde::{Deserialize, Serialize};

pub const FNV_OFFSET_32: u32 = 0x811c_9dc5;
pub const FNV_PRIME_32: u32 = 0x0100_0193;
pub const FNV_OFFSET_64: u64 = 0xcbf2_9ce4_8422_2325;
pub const FNV_PRIME_64: u64 = 0x0000_0100_0000_01b3;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Checksum {
    #[default]
    Crc32,
    Fnv1,
    Fnv1a,
    Fnv1a64,
    Murmur3,
    XorSum,
}

impl Checksum {
    pub fn compute(self, bytes: &[u8], seed: u32) -> u64 {
        match self {
            Checksum::Crc32 => crc32(bytes) as u64,
            Checksum::Fnv1 => fnv1_32(bytes) as u64,
            Checksum::Fnv1a => fnv1a_32(bytes) as u64,
            Checksum::Fnv1a64 => fnv1a_64(bytes),
            Checksum::Murmur3 => murmur3_x86_32(bytes, seed) as u64,
            Checksum::XorSum => xor_sum(bytes, seed, true) as u64,
        }
    }

    pub fn name(self) -> &'static str {
        match self {
            Checksum::Crc32 => "crc32",
            Checksum::Fnv1 => "fnv1",
            Checksum::Fnv1a => "fnv1a",
            Checksum::Fnv1a64 => "fnv1a64",
            Checksum::Murmur3 => "murmur3",
            Checksum::XorSum => "xor-sum",
        }
    }
}

pub fn crc32(bytes: &[u8]) -> u32 {
    let mut hasher = crc32fast::Hasher::new();
    hasher.update(bytes);
    hasher.finalize()
}

pub fn fnv1_32(bytes: &[u8]) -> u32 {
    let mut hash = FNV_OFFSET_32;
    for byte in bytes {
        hash = hash.wrapping_mul(FNV_PRIME_32) ^ u32::from(*byte);
    }
    hash
}

pub fn fnv1a_32(bytes: &[u8]) -> u32 {
    let mut hash = FNV_OFFSET_32;
    for byte in bytes {
        hash = (hash ^ u32::from(*byte)).wrapping_mul(FNV_PRIME_32);
    }
    hash
}

pub fn fnv1a_64(bytes: &[u8]) -> u64 {
    let mut hash = FNV_OFFSET_64;
    for byte in bytes {
        hash = (hash ^ u64::from(*byte)).wrapping_mul(FNV_PRIME_64);
    }
    hash
}

pub fn murmur3_x86_32(bytes: &[u8], seed: u32) -> u32 {
    const C1: u32 = 0xcc9e_2d51;
    const C2: u32 = 0x1b87_3593;

    let mut hash = seed;
    let mut chunks = bytes.chunks_exact(4);

    for chunk in &mut chunks {
        let mut block = u32::from_le_bytes([chunk[0], chunk[1], chunk[2], chunk[3]]);
        block = block.wrapping_mul(C1).rotate_left(15).wrapping_mul(C2);
        hash ^= block;
        hash = hash.rotate_left(13).wrapping_mul(5).wrapping_add(0xe654_6b64);
    }

    let tail = chunks.remainder();
    if !tail.is_empty() {
        let mut block = 0u32;
        for (index, byte) in tail.iter().enumerate() {
            block |= u32::from(*byte) << (8 * index);
        }
        block = block.wrapping_mul(C1).rotate_left(15).wrapping_mul(C2);
        hash ^= block;
    }

    hash ^= bytes.len() as u32;
    hash ^= hash >> 16;
    hash = hash.wrapping_mul(0x85eb_ca6b);
    hash ^= hash >> 13;
    hash = hash.wrapping_mul(0xc2b2_ae35);
    hash ^= hash >> 16;
    hash
}

pub fn murmur3_skipping_whitespace(bytes: &[u8], seed: u32) -> u32 {
    let filtered: Vec<u8> = bytes
        .iter()
        .copied()
        .filter(|byte| !byte.is_ascii_whitespace())
        .collect();
    murmur3_x86_32(&filtered, seed)
}

pub fn xor_sum(bytes: &[u8], seed: u32, ascii_only: bool) -> u32 {
    let total: u32 = bytes
        .iter()
        .filter(|byte| !ascii_only || **byte < 128)
        .fold(0u32, |acc, byte| acc.wrapping_add(u32::from(*byte)));
    seed ^ total
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn murmur3_matches_the_reference_vectors() {
        assert_eq!(murmur3_x86_32(b"", 0), 0);
        assert_eq!(murmur3_x86_32(b"", 1), 0x514e_28b7);
        assert_eq!(murmur3_x86_32(b"", 0xffff_ffff), 0x81f1_6f39);
        assert_eq!(murmur3_x86_32(b"hello", 0), 0x248b_fa47);
        assert_eq!(murmur3_x86_32(b"The quick brown fox jumps over the lazy dog", 0), 0x2e4f_f723);
    }

    #[test]
    fn fnv_matches_the_reference_vectors() {
        assert_eq!(fnv1a_32(b""), FNV_OFFSET_32);
        assert_eq!(fnv1a_32(b"a"), 0xe40c_292c);
        assert_eq!(fnv1a_32(b"foobar"), 0xbf9c_f968);
        assert_eq!(fnv1_32(b"a"), 0x050c_5d7e);
        assert_eq!(fnv1a_64(b"a"), 0xaf63_dc4c_8601_ec8c);
    }

    #[test]
    fn whitespace_skipping_ignores_layout_only_edits() {
        let dense = b"function f(){return 1}";
        let spaced = b"function f ( ) {\n  return 1\n}";
        assert_eq!(
            murmur3_skipping_whitespace(dense, 35_549),
            murmur3_skipping_whitespace(spaced, 35_549)
        );
        assert_ne!(murmur3_x86_32(dense, 35_549), murmur3_x86_32(spaced, 35_549));
    }

    #[test]
    fn the_xor_sum_ignores_high_bytes_when_asked() {
        assert_eq!(xor_sum(b"abc", 24, true), 24 ^ (97 + 98 + 99));
        assert_eq!(xor_sum(&[97, 200], 24, true), 24 ^ 97);
        assert_eq!(xor_sum(&[97, 200], 24, false), 24 ^ (97 + 200));
    }

    #[test]
    fn the_enum_dispatches_to_the_same_functions() {
        let bytes = b"payload";
        assert_eq!(Checksum::Crc32.compute(bytes, 0), crc32(bytes) as u64);
        assert_eq!(Checksum::Murmur3.compute(bytes, 7), murmur3_x86_32(bytes, 7) as u64);
        assert_eq!(Checksum::Fnv1a64.compute(bytes, 0), fnv1a_64(bytes));
    }
}
