use sha2::{Digest, Sha256};

pub fn sha256(bytes: &[u8]) -> String {
    let mut hasher = Sha256::new();
    hasher.update(bytes);
    hex::encode(hasher.finalize())
}

pub fn sha256_short(bytes: &[u8]) -> String {
    sha256(bytes)[..16].to_string()
}

pub fn fnv1a32(bytes: &[u8]) -> u32 {
    let mut hash: u32 = 0x811c9dc5;
    for byte in bytes {
        hash ^= u32::from(*byte);
        hash = hash.wrapping_mul(0x0100_0193);
    }
    hash
}

pub fn fnv1a64(bytes: &[u8]) -> u64 {
    let mut hash: u64 = 0xcbf2_9ce4_8422_2325;
    for byte in bytes {
        hash ^= u64::from(*byte);
        hash = hash.wrapping_mul(0x0000_0100_0000_01b3);
    }
    hash
}

pub fn djb2(bytes: &[u8]) -> u32 {
    let mut hash: u32 = 5381;
    for byte in bytes {
        hash = hash.wrapping_mul(33).wrapping_add(u32::from(*byte));
    }
    hash
}

pub fn sdbm(bytes: &[u8]) -> u32 {
    let mut hash: u32 = 0;
    for byte in bytes {
        hash = u32::from(*byte)
            .wrapping_add(hash << 6)
            .wrapping_add(hash << 16)
            .wrapping_sub(hash);
    }
    hash
}

pub fn java_string_hash(text: &str) -> i32 {
    let mut hash: i32 = 0;
    for unit in text.encode_utf16() {
        hash = hash.wrapping_mul(31).wrapping_add(i32::from(unit));
    }
    hash
}

pub fn murmur3_32(bytes: &[u8], seed: u32) -> u32 {
    const C1: u32 = 0xcc9e_2d51;
    const C2: u32 = 0x1b87_3593;

    let mut hash = seed;
    let chunks = bytes.chunks_exact(4);
    let tail = chunks.remainder();

    for chunk in chunks {
        let mut k = u32::from_le_bytes([chunk[0], chunk[1], chunk[2], chunk[3]]);
        k = k.wrapping_mul(C1);
        k = k.rotate_left(15);
        k = k.wrapping_mul(C2);
        hash ^= k;
        hash = hash.rotate_left(13);
        hash = hash.wrapping_mul(5).wrapping_add(0xe654_6b64);
    }

    let mut k: u32 = 0;
    for (index, byte) in tail.iter().enumerate() {
        k |= u32::from(*byte) << (8 * index);
    }
    if !tail.is_empty() {
        k = k.wrapping_mul(C1);
        k = k.rotate_left(15);
        k = k.wrapping_mul(C2);
        hash ^= k;
    }

    hash ^= bytes.len() as u32;
    hash ^= hash >> 16;
    hash = hash.wrapping_mul(0x85eb_ca6b);
    hash ^= hash >> 13;
    hash = hash.wrapping_mul(0xc2b2_ae35);
    hash ^= hash >> 16;
    hash
}

pub fn crc32(bytes: &[u8]) -> u32 {
    let mut table = [0u32; 256];
    for (index, entry) in table.iter_mut().enumerate() {
        let mut value = index as u32;
        for _ in 0..8 {
            value = if value & 1 == 1 { 0xedb8_8320 ^ (value >> 1) } else { value >> 1 };
        }
        *entry = value;
    }

    let mut crc = 0xffff_ffffu32;
    for byte in bytes {
        crc = table[((crc ^ u32::from(*byte)) & 0xff) as usize] ^ (crc >> 8);
    }
    crc ^ 0xffff_ffff
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HashKind {
    Fnv1a32,
    Djb2,
    Sdbm,
    Crc32,
    Murmur3,
    Java,
    Sum,
}

impl HashKind {
    pub const ALL: [HashKind; 7] = [
        HashKind::Fnv1a32,
        HashKind::Djb2,
        HashKind::Sdbm,
        HashKind::Crc32,
        HashKind::Murmur3,
        HashKind::Java,
        HashKind::Sum,
    ];

    pub fn name(self) -> &'static str {
        match self {
            HashKind::Fnv1a32 => "fnv1a32",
            HashKind::Djb2 => "djb2",
            HashKind::Sdbm => "sdbm",
            HashKind::Crc32 => "crc32",
            HashKind::Murmur3 => "murmur3",
            HashKind::Java => "java",
            HashKind::Sum => "sum",
        }
    }

    pub fn apply(self, text: &str) -> u32 {
        let bytes = text.as_bytes();
        match self {
            HashKind::Fnv1a32 => fnv1a32(bytes),
            HashKind::Djb2 => djb2(bytes),
            HashKind::Sdbm => sdbm(bytes),
            HashKind::Crc32 => crc32(bytes),
            HashKind::Murmur3 => murmur3_32(bytes, 0),
            HashKind::Java => java_string_hash(text) as u32,
            HashKind::Sum => bytes.iter().fold(0u32, |acc, b| acc.wrapping_add(u32::from(*b))),
        }
    }
}
