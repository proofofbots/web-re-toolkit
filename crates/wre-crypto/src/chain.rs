use wre_core::error::{Error, Result};

use crate::block::BlockCipher;

pub trait Order: Send {
    fn reset(&mut self, blocks: usize);
    fn pick(&mut self, pending: usize, previous: Option<&[u8]>) -> usize;

    fn name(&self) -> &'static str {
        "order"
    }
}

#[derive(Debug, Clone, Default)]
pub struct Sequential;

impl Order for Sequential {
    fn reset(&mut self, _blocks: usize) {}

    fn pick(&mut self, _pending: usize, _previous: Option<&[u8]>) -> usize {
        0
    }

    fn name(&self) -> &'static str {
        "sequential"
    }
}

#[derive(Debug, Clone)]
pub struct WindowCursor {
    window: usize,
    cursor: usize,
    word_offset: usize,
}

impl WindowCursor {
    pub fn new(window: usize) -> Self {
        Self { window: window.max(1), cursor: 0, word_offset: 0 }
    }

    pub fn with_word_offset(window: usize, word_offset: usize) -> Self {
        Self { window: window.max(1), cursor: 0, word_offset }
    }

    pub fn window(&self) -> usize {
        self.window
    }
}

impl Order for WindowCursor {
    fn reset(&mut self, _blocks: usize) {
        self.cursor = 0;
    }

    fn pick(&mut self, pending: usize, previous: Option<&[u8]>) -> usize {
        let reachable = pending.min(self.window);
        if reachable <= 1 {
            self.cursor = 0;
            return 0;
        }

        let step = previous
            .filter(|block| block.len() >= self.word_offset + 4)
            .map(|block| {
                let start = self.word_offset;
                u32::from_be_bytes([
                    block[start],
                    block[start + 1],
                    block[start + 2],
                    block[start + 3],
                ]) as usize
            })
            .unwrap_or(0);

        self.cursor = (self.cursor + step) % reachable;
        self.cursor
    }

    fn name(&self) -> &'static str {
        "window-cursor"
    }
}

pub fn split_blocks(data: &[u8], size: usize) -> Result<Vec<Vec<u8>>> {
    if size == 0 {
        return Err(Error::msg("block size must not be zero"));
    }
    if data.len() % size != 0 {
        return Err(Error::msg(format!(
            "{} bytes is not a whole number of {size} byte blocks",
            data.len()
        )));
    }
    Ok(data.chunks(size).map(<[u8]>::to_vec).collect())
}

pub struct Ecb;

impl Ecb {
    pub fn seal(cipher: &dyn BlockCipher, plaintext: &[u8]) -> Result<Vec<u8>> {
        let mut blocks = split_blocks(plaintext, cipher.block_size())?;
        for block in &mut blocks {
            cipher.encrypt_block(block);
        }
        Ok(blocks.concat())
    }

    pub fn open(cipher: &dyn BlockCipher, ciphertext: &[u8]) -> Result<Vec<u8>> {
        let mut blocks = split_blocks(ciphertext, cipher.block_size())?;
        for block in &mut blocks {
            cipher.decrypt_block(block);
        }
        Ok(blocks.concat())
    }
}

pub struct Cbc;

impl Cbc {
    pub fn seal(
        cipher: &dyn BlockCipher,
        order: &mut dyn Order,
        iv: &[u8],
        plaintext: &[u8],
    ) -> Result<Vec<u8>> {
        let size = cipher.block_size();
        check_iv(iv, size)?;

        let mut pending = split_blocks(plaintext, size)?;
        order.reset(pending.len());

        let mut previous = iv.to_vec();
        let mut out = Vec::with_capacity(plaintext.len());
        let mut emitted: Vec<usize> = Vec::with_capacity(pending.len());
        let mut indices: Vec<usize> = (0..pending.len()).collect();

        while !pending.is_empty() {
            let choice = order.pick(pending.len(), emitted.last().map(|_| previous.as_slice()));
            let choice = choice.min(pending.len() - 1);

            let mut block = pending.remove(choice);
            let source = indices.remove(choice);

            for (byte, mask) in block.iter_mut().zip(previous.iter()) {
                *byte ^= *mask;
            }
            cipher.encrypt_block(&mut block);

            previous = block.clone();
            out.extend_from_slice(&block);
            emitted.push(source);
        }

        Ok(out)
    }

    pub fn open(
        cipher: &dyn BlockCipher,
        order: &mut dyn Order,
        iv: &[u8],
        ciphertext: &[u8],
    ) -> Result<Vec<u8>> {
        let size = cipher.block_size();
        check_iv(iv, size)?;

        let blocks = split_blocks(ciphertext, size)?;
        order.reset(blocks.len());

        let mut slots: Vec<Option<Vec<u8>>> = vec![None; blocks.len()];
        let mut free: Vec<usize> = (0..blocks.len()).collect();
        let mut previous = iv.to_vec();
        let mut step = 0usize;

        for block in &blocks {
            let choice = order.pick(free.len(), if step == 0 { None } else { Some(&previous) });
            let choice = choice.min(free.len() - 1);
            let slot = free.remove(choice);

            let mut plain = block.clone();
            cipher.decrypt_block(&mut plain);
            for (byte, mask) in plain.iter_mut().zip(previous.iter()) {
                *byte ^= *mask;
            }

            slots[slot] = Some(plain);
            previous = block.clone();
            step += 1;
        }

        let mut out = Vec::with_capacity(ciphertext.len());
        for slot in slots {
            match slot {
                Some(block) => out.extend_from_slice(&block),
                None => return Err(Error::msg("chain order did not fill every block")),
            }
        }

        Ok(out)
    }
}

pub fn ctr_apply(cipher: &dyn BlockCipher, nonce: &[u8], data: &[u8]) -> Result<Vec<u8>> {
    let size = cipher.block_size();
    check_iv(nonce, size)?;

    let mut counter = nonce.to_vec();
    let mut out = Vec::with_capacity(data.len());

    for chunk in data.chunks(size) {
        let mut keystream = counter.clone();
        cipher.encrypt_block(&mut keystream);
        for (index, byte) in chunk.iter().enumerate() {
            out.push(byte ^ keystream[index]);
        }
        increment(&mut counter);
    }

    Ok(out)
}

fn increment(counter: &mut [u8]) {
    for byte in counter.iter_mut().rev() {
        let (next, carried) = byte.overflowing_add(1);
        *byte = next;
        if !carried {
            return;
        }
    }
}

fn check_iv(iv: &[u8], size: usize) -> Result<()> {
    if iv.len() == size {
        return Ok(());
    }
    Err(Error::msg(format!(
        "iv is {} bytes, the cipher takes {size}",
        iv.len()
    )))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::block::Xtea;

    fn cipher() -> Xtea {
        Xtea::new(b"0123456789abcdef").unwrap()
    }

    #[test]
    fn ecb_round_trips() {
        let cipher = cipher();
        let plaintext = b"sixteen bytes!!!".to_vec();
        let sealed = Ecb::seal(&cipher, &plaintext).unwrap();
        assert_eq!(Ecb::open(&cipher, &sealed).unwrap(), plaintext);
    }

    #[test]
    fn cbc_round_trips_in_order() {
        let cipher = cipher();
        let plaintext = b"twenty four bytes long!!".to_vec();
        let iv = [9u8; 8];

        let sealed = Cbc::seal(&cipher, &mut Sequential, &iv, &plaintext).unwrap();
        let opened = Cbc::open(&cipher, &mut Sequential, &iv, &sealed).unwrap();
        assert_eq!(opened, plaintext);
    }

    #[test]
    fn the_window_cursor_reorders_and_still_inverts() {
        let cipher = cipher();
        let plaintext: Vec<u8> = (0..80u8).collect();
        let iv = [3u8; 8];

        let ordered = Cbc::seal(&cipher, &mut Sequential, &iv, &plaintext).unwrap();
        let shuffled = Cbc::seal(&cipher, &mut WindowCursor::new(5), &iv, &plaintext).unwrap();
        assert_ne!(ordered, shuffled);

        let opened = Cbc::open(&cipher, &mut WindowCursor::new(5), &iv, &shuffled).unwrap();
        assert_eq!(opened, plaintext);
    }

    #[test]
    fn a_window_of_one_degrades_to_sequential() {
        let cipher = cipher();
        let plaintext: Vec<u8> = (0..40u8).collect();
        let iv = [1u8; 8];

        let ordered = Cbc::seal(&cipher, &mut Sequential, &iv, &plaintext).unwrap();
        let windowed = Cbc::seal(&cipher, &mut WindowCursor::new(1), &iv, &plaintext).unwrap();
        assert_eq!(ordered, windowed);
    }

    #[test]
    fn ctr_is_its_own_inverse() {
        let cipher = cipher();
        let data = b"any length at all, not a multiple".to_vec();
        let nonce = [4u8; 8];

        let sealed = ctr_apply(&cipher, &nonce, &data).unwrap();
        assert_eq!(ctr_apply(&cipher, &nonce, &sealed).unwrap(), data);
    }

    #[test]
    fn a_ragged_body_is_rejected() {
        let cipher = cipher();
        assert!(Ecb::seal(&cipher, b"five!").is_err());
        assert!(Cbc::seal(&cipher, &mut Sequential, &[0u8; 4], b"eight by").is_err());
    }
}
