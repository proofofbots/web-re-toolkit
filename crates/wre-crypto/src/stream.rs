use wre_core::error::{Error, Result};

pub fn xor_repeating(data: &[u8], key: &[u8]) -> Result<Vec<u8>> {
    if key.is_empty() {
        return Err(Error::msg("an xor key must not be empty"));
    }

    Ok(data
        .iter()
        .enumerate()
        .map(|(index, byte)| byte ^ key[index % key.len()])
        .collect())
}

pub fn xor_indexed(data: &[u8], key: &[u8], modulus: usize) -> Result<Vec<u8>> {
    if key.is_empty() {
        return Err(Error::msg("an xor key must not be empty"));
    }
    if modulus == 0 {
        return Err(Error::msg("the index modulus must not be zero"));
    }

    Ok(data
        .iter()
        .enumerate()
        .map(|(index, byte)| {
            let step = (index % modulus) as u8;
            byte ^ key[index % key.len()].wrapping_add(step)
        })
        .collect())
}

#[derive(Debug, Clone)]
pub struct Rc4 {
    state: [u8; 256],
    left: u8,
    right: u8,
}

impl Rc4 {
    pub fn new(key: &[u8]) -> Result<Self> {
        if key.is_empty() {
            return Err(Error::msg("an rc4 key must not be empty"));
        }

        let mut state = [0u8; 256];
        for (index, slot) in state.iter_mut().enumerate() {
            *slot = index as u8;
        }

        let mut swap = 0u8;
        for index in 0..256 {
            swap = swap
                .wrapping_add(state[index])
                .wrapping_add(key[index % key.len()]);
            state.swap(index, swap as usize);
        }

        Ok(Self { state, left: 0, right: 0 })
    }

    pub fn skip(&mut self, bytes: usize) {
        for _ in 0..bytes {
            self.byte();
        }
    }

    pub fn apply(&mut self, data: &[u8]) -> Vec<u8> {
        data.iter().map(|byte| byte ^ self.byte()).collect()
    }

    fn byte(&mut self) -> u8 {
        self.left = self.left.wrapping_add(1);
        self.right = self.right.wrapping_add(self.state[self.left as usize]);
        self.state.swap(self.left as usize, self.right as usize);

        let index = self.state[self.left as usize].wrapping_add(self.state[self.right as usize]);
        self.state[index as usize]
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn xor_is_its_own_inverse() {
        let key = b"omgtopkek";
        let sealed = xor_repeating(b"{\"check\":\"ok\"}", key).unwrap();
        assert_eq!(xor_repeating(&sealed, key).unwrap(), b"{\"check\":\"ok\"}");
    }

    #[test]
    fn an_empty_key_is_rejected() {
        assert!(xor_repeating(b"data", b"").is_err());
        assert!(xor_indexed(b"data", b"k", 0).is_err());
        assert!(Rc4::new(b"").is_err());
    }

    #[test]
    fn the_indexed_variant_differs_from_the_plain_one_and_inverts() {
        let key = b"key";
        let plain = b"the quick brown fox jumps over it";

        let plain_xor = xor_repeating(plain, key).unwrap();
        let indexed = xor_indexed(plain, key, 23).unwrap();
        assert_ne!(plain_xor, indexed);

        let back = xor_indexed(&indexed, key, 23).unwrap();
        assert_eq!(back, plain);
    }

    #[test]
    fn rc4_matches_the_reference_vector() {
        let mut cipher = Rc4::new(b"Key").unwrap();
        assert_eq!(hex::encode(cipher.apply(b"Plaintext")), "bbf316e8d940af0ad3");

        let mut cipher = Rc4::new(b"Secret").unwrap();
        assert_eq!(hex::encode(cipher.apply(b"Attack at dawn")), "45a01f645fc35b383552544b9bf5");
    }

    #[test]
    fn rc4_round_trips() {
        let plain = b"the payload body".to_vec();
        let sealed = Rc4::new(b"seed").unwrap().apply(&plain);
        assert_eq!(Rc4::new(b"seed").unwrap().apply(&sealed), plain);
    }
}
