use aes::cipher::{BlockCipherDecrypt, BlockCipherEncrypt, KeyInit};
use serde::{Deserialize, Serialize};

use wre_core::error::{Error, Result};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Endian {
    #[default]
    Big,
    Little,
}

impl Endian {
    pub fn read_u32(self, bytes: &[u8]) -> u32 {
        let word = [bytes[0], bytes[1], bytes[2], bytes[3]];
        match self {
            Endian::Big => u32::from_be_bytes(word),
            Endian::Little => u32::from_le_bytes(word),
        }
    }

    pub fn write_u32(self, value: u32, out: &mut [u8]) {
        let word = match self {
            Endian::Big => value.to_be_bytes(),
            Endian::Little => value.to_le_bytes(),
        };
        out[..4].copy_from_slice(&word);
    }
}

pub trait BlockCipher: Send + Sync {
    fn block_size(&self) -> usize;
    fn encrypt_block(&self, block: &mut [u8]);
    fn decrypt_block(&self, block: &mut [u8]);

    fn name(&self) -> &'static str {
        "block"
    }
}

pub const XTEA_DELTA: u32 = 0x9E37_79B9;

#[derive(Debug, Clone)]
pub struct Xtea {
    key: [u32; 4],
    rounds: u32,
    delta: u32,
    endian: Endian,
}

impl Xtea {
    pub fn new(key: &[u8]) -> Result<Self> {
        Self::with(key, 32, XTEA_DELTA, Endian::Big)
    }

    pub fn with(key: &[u8], rounds: u32, delta: u32, endian: Endian) -> Result<Self> {
        if key.len() != 16 {
            return Err(Error::msg(format!(
                "xtea needs a 16 byte key, got {}",
                key.len()
            )));
        }

        let mut words = [0u32; 4];
        for (index, word) in words.iter_mut().enumerate() {
            *word = endian.read_u32(&key[index * 4..]);
        }

        Ok(Self { key: words, rounds, delta, endian })
    }

    pub fn rounds(&self) -> u32 {
        self.rounds
    }
}

impl BlockCipher for Xtea {
    fn block_size(&self) -> usize {
        8
    }

    fn name(&self) -> &'static str {
        "xtea"
    }

    fn encrypt_block(&self, block: &mut [u8]) {
        let mut left = self.endian.read_u32(block);
        let mut right = self.endian.read_u32(&block[4..]);
        let mut sum = 0u32;

        for _ in 0..self.rounds {
            let mix = ((right << 4) ^ (right >> 5)).wrapping_add(right);
            let subkey = self.key[(sum & 3) as usize];
            left = left.wrapping_add(mix ^ sum.wrapping_add(subkey));

            sum = sum.wrapping_add(self.delta);

            let mix = ((left << 4) ^ (left >> 5)).wrapping_add(left);
            let subkey = self.key[((sum >> 11) & 3) as usize];
            right = right.wrapping_add(mix ^ sum.wrapping_add(subkey));
        }

        self.endian.write_u32(left, block);
        self.endian.write_u32(right, &mut block[4..]);
    }

    fn decrypt_block(&self, block: &mut [u8]) {
        let mut left = self.endian.read_u32(block);
        let mut right = self.endian.read_u32(&block[4..]);
        let mut sum = self.delta.wrapping_mul(self.rounds);

        for _ in 0..self.rounds {
            let mix = ((left << 4) ^ (left >> 5)).wrapping_add(left);
            let subkey = self.key[((sum >> 11) & 3) as usize];
            right = right.wrapping_sub(mix ^ sum.wrapping_add(subkey));

            sum = sum.wrapping_sub(self.delta);

            let mix = ((right << 4) ^ (right >> 5)).wrapping_add(right);
            let subkey = self.key[(sum & 3) as usize];
            left = left.wrapping_sub(mix ^ sum.wrapping_add(subkey));
        }

        self.endian.write_u32(left, block);
        self.endian.write_u32(right, &mut block[4..]);
    }
}

#[derive(Debug, Clone)]
pub struct Tea {
    key: [u32; 4],
    rounds: u32,
    delta: u32,
    endian: Endian,
}

impl Tea {
    pub fn new(key: &[u8]) -> Result<Self> {
        Self::with(key, 32, XTEA_DELTA, Endian::Big)
    }

    pub fn with(key: &[u8], rounds: u32, delta: u32, endian: Endian) -> Result<Self> {
        if key.len() != 16 {
            return Err(Error::msg(format!(
                "tea needs a 16 byte key, got {}",
                key.len()
            )));
        }

        let mut words = [0u32; 4];
        for (index, word) in words.iter_mut().enumerate() {
            *word = endian.read_u32(&key[index * 4..]);
        }

        Ok(Self { key: words, rounds, delta, endian })
    }
}

impl BlockCipher for Tea {
    fn block_size(&self) -> usize {
        8
    }

    fn name(&self) -> &'static str {
        "tea"
    }

    fn encrypt_block(&self, block: &mut [u8]) {
        let mut left = self.endian.read_u32(block);
        let mut right = self.endian.read_u32(&block[4..]);
        let mut sum = 0u32;

        for _ in 0..self.rounds {
            sum = sum.wrapping_add(self.delta);
            left = left.wrapping_add(
                ((right << 4).wrapping_add(self.key[0]))
                    ^ right.wrapping_add(sum)
                    ^ ((right >> 5).wrapping_add(self.key[1])),
            );
            right = right.wrapping_add(
                ((left << 4).wrapping_add(self.key[2]))
                    ^ left.wrapping_add(sum)
                    ^ ((left >> 5).wrapping_add(self.key[3])),
            );
        }

        self.endian.write_u32(left, block);
        self.endian.write_u32(right, &mut block[4..]);
    }

    fn decrypt_block(&self, block: &mut [u8]) {
        let mut left = self.endian.read_u32(block);
        let mut right = self.endian.read_u32(&block[4..]);
        let mut sum = self.delta.wrapping_mul(self.rounds);

        for _ in 0..self.rounds {
            right = right.wrapping_sub(
                ((left << 4).wrapping_add(self.key[2]))
                    ^ left.wrapping_add(sum)
                    ^ ((left >> 5).wrapping_add(self.key[3])),
            );
            left = left.wrapping_sub(
                ((right << 4).wrapping_add(self.key[0]))
                    ^ right.wrapping_add(sum)
                    ^ ((right >> 5).wrapping_add(self.key[1])),
            );
            sum = sum.wrapping_sub(self.delta);
        }

        self.endian.write_u32(left, block);
        self.endian.write_u32(right, &mut block[4..]);
    }
}

pub struct Aes128(aes::Aes128);

impl Aes128 {
    pub fn new(key: &[u8]) -> Result<Self> {
        if key.len() != 16 {
            return Err(Error::msg(format!(
                "aes-128 needs a 16 byte key, got {}",
                key.len()
            )));
        }
        let mut material = [0u8; 16];
        material.copy_from_slice(key);
        Ok(Self(aes::Aes128::new(&material.into())))
    }
}

impl BlockCipher for Aes128 {
    fn block_size(&self) -> usize {
        16
    }

    fn name(&self) -> &'static str {
        "aes-128"
    }

    fn encrypt_block(&self, block: &mut [u8]) {
        let mut buffer = [0u8; 16];
        let width = block.len().min(16);
        buffer[..width].copy_from_slice(&block[..width]);

        let mut array = aes::Block::from(buffer);
        self.0.encrypt_block(&mut array);
        block[..width].copy_from_slice(&array[..width]);
    }

    fn decrypt_block(&self, block: &mut [u8]) {
        let mut buffer = [0u8; 16];
        let width = block.len().min(16);
        buffer[..width].copy_from_slice(&block[..width]);

        let mut array = aes::Block::from(buffer);
        self.0.decrypt_block(&mut array);
        block[..width].copy_from_slice(&array[..width]);
    }
}

pub struct Aes256(aes::Aes256);

impl Aes256 {
    pub fn new(key: &[u8]) -> Result<Self> {
        if key.len() != 32 {
            return Err(Error::msg(format!(
                "aes-256 needs a 32 byte key, got {}",
                key.len()
            )));
        }
        let mut material = [0u8; 32];
        material.copy_from_slice(key);
        Ok(Self(aes::Aes256::new(&material.into())))
    }
}

impl BlockCipher for Aes256 {
    fn block_size(&self) -> usize {
        16
    }

    fn name(&self) -> &'static str {
        "aes-256"
    }

    fn encrypt_block(&self, block: &mut [u8]) {
        let mut buffer = [0u8; 16];
        let width = block.len().min(16);
        buffer[..width].copy_from_slice(&block[..width]);

        let mut array = aes::Block::from(buffer);
        self.0.encrypt_block(&mut array);
        block[..width].copy_from_slice(&array[..width]);
    }

    fn decrypt_block(&self, block: &mut [u8]) {
        let mut buffer = [0u8; 16];
        let width = block.len().min(16);
        buffer[..width].copy_from_slice(&block[..width]);

        let mut array = aes::Block::from(buffer);
        self.0.decrypt_block(&mut array);
        block[..width].copy_from_slice(&array[..width]);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn xtea_round_trips() {
        let cipher = Xtea::new(b"0123456789abcdef").unwrap();
        let mut block = *b"plaintxt";
        cipher.encrypt_block(&mut block);
        assert_ne!(&block, b"plaintxt");
        cipher.decrypt_block(&mut block);
        assert_eq!(&block, b"plaintxt");
    }

    #[test]
    fn xtea_matches_the_reference_vector() {
        let cipher = Xtea::with(&[0u8; 16], 32, XTEA_DELTA, Endian::Big).unwrap();
        let mut block = [0u8; 8];
        cipher.encrypt_block(&mut block);
        assert_eq!(hex::encode(block), "dee9d4d8f7131ed9");
    }

    #[test]
    fn tea_round_trips() {
        let cipher = Tea::new(b"0123456789abcdef").unwrap();
        let mut block = *b"plaintxt";
        cipher.encrypt_block(&mut block);
        cipher.decrypt_block(&mut block);
        assert_eq!(&block, b"plaintxt");
    }

    #[test]
    fn aes_round_trips_both_widths() {
        let small = Aes128::new(&[7u8; 16]).unwrap();
        let mut block = [1u8; 16];
        small.encrypt_block(&mut block);
        small.decrypt_block(&mut block);
        assert_eq!(block, [1u8; 16]);

        let large = Aes256::new(&[7u8; 32]).unwrap();
        let mut block = [2u8; 16];
        large.encrypt_block(&mut block);
        large.decrypt_block(&mut block);
        assert_eq!(block, [2u8; 16]);
    }

    #[test]
    fn wrong_key_lengths_are_rejected() {
        assert!(Xtea::new(b"short").is_err());
        assert!(Aes128::new(&[0u8; 8]).is_err());
        assert!(Aes256::new(&[0u8; 16]).is_err());
    }

    #[test]
    fn endianness_changes_the_ciphertext() {
        let big = Xtea::with(b"0123456789abcdef", 32, XTEA_DELTA, Endian::Big).unwrap();
        let little = Xtea::with(b"0123456789abcdef", 32, XTEA_DELTA, Endian::Little).unwrap();

        let mut one = *b"plaintxt";
        let mut two = *b"plaintxt";
        big.encrypt_block(&mut one);
        little.encrypt_block(&mut two);
        assert_ne!(one, two);
    }
}
