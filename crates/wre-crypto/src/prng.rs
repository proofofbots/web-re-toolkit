use serde::{Deserialize, Serialize};

pub trait Rng {
    fn next_u64(&mut self) -> u64;

    fn draw(&mut self, modulus: u64) -> u64 {
        if modulus == 0 { 0 } else { self.next_u64() % modulus }
    }

    fn take(&mut self, count: usize) -> Vec<u64> {
        (0..count).map(|_| self.next_u64()).collect()
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Lcg {
    pub state: u64,
    pub multiplier: u64,
    pub increment: u64,
    pub product_mask: u64,
    pub state_mask: u64,
    pub output_shift: u32,
    pub output_mask: u64,
}

impl Lcg {
    pub fn new(seed: u64, multiplier: u64, increment: u64, state_mask: u64) -> Self {
        Self {
            state: seed & state_mask,
            multiplier,
            increment,
            product_mask: u32::MAX as u64,
            state_mask,
            output_shift: 0,
            output_mask: state_mask,
        }
    }

    pub fn output(mut self, shift: u32, mask: u64) -> Self {
        self.output_shift = shift;
        self.output_mask = mask;
        self
    }

    pub fn product_mask(mut self, mask: u64) -> Self {
        self.product_mask = mask;
        self
    }

    pub fn seed(&mut self, seed: u64) {
        self.state = seed & self.state_mask;
    }

    pub fn step(&mut self) -> u64 {
        let product = self.state.wrapping_mul(self.multiplier) & self.product_mask;
        self.state = product.wrapping_add(self.increment) & self.state_mask;
        self.state
    }
}

impl Rng for Lcg {
    fn next_u64(&mut self) -> u64 {
        let state = self.step();
        (state >> self.output_shift) & self.output_mask
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct XorShift32 {
    pub state: u32,
}

impl XorShift32 {
    pub fn new(seed: u32) -> Self {
        Self { state: if seed == 0 { 0x1a2b_3c4d } else { seed } }
    }
}

impl Rng for XorShift32 {
    fn next_u64(&mut self) -> u64 {
        self.state ^= self.state << 13;
        self.state ^= self.state >> 17;
        self.state ^= self.state << 5;
        self.state as u64
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Mulberry32 {
    pub state: u32,
}

impl Mulberry32 {
    pub fn new(seed: u32) -> Self {
        Self { state: seed }
    }
}

impl Rng for Mulberry32 {
    fn next_u64(&mut self) -> u64 {
        self.state = self.state.wrapping_add(0x6D2B_79F5);
        let mut value = self.state;
        value = (value ^ (value >> 15)).wrapping_mul(value | 1);
        value ^= value.wrapping_add((value ^ (value >> 7)).wrapping_mul(value | 61));
        (value ^ (value >> 14)) as u64
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SplitMix64 {
    pub state: u64,
}

impl SplitMix64 {
    pub fn new(seed: u64) -> Self {
        Self { state: seed }
    }
}

impl Rng for SplitMix64 {
    fn next_u64(&mut self) -> u64 {
        self.state = self.state.wrapping_add(0x9E37_79B9_7F4A_7C15);
        let mut value = self.state;
        value = (value ^ (value >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
        value = (value ^ (value >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
        value ^ (value >> 31)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_lcg_is_reproducible_from_its_seed() {
        let mut one = Lcg::new(7, 65_793, 4_282_663, 0x7f_ffff).output(8, 0xffff);
        let mut two = one.clone();
        assert_eq!(one.take(16), two.take(16));
    }

    #[test]
    fn lcg_output_stays_inside_its_mask() {
        let mut rng = Lcg::new(1, 65_793, 4_282_663, 0x7f_ffff).output(8, 0xffff);
        for value in rng.take(512) {
            assert!(value <= 0xffff);
        }
    }

    #[test]
    fn reseeding_restarts_the_stream() {
        let mut rng = Lcg::new(3, 65_793, 4_282_663, 0x7f_ffff).output(8, 0xffff);
        let first = rng.take(8);
        rng.seed(3);
        assert_eq!(rng.take(8), first);
    }

    #[test]
    fn the_generators_do_not_stall_on_zero() {
        assert_ne!(XorShift32::new(0).next_u64(), 0);
        assert_ne!(SplitMix64::new(0).next_u64(), 0);
        assert_ne!(Mulberry32::new(0).next_u64(), 0);
    }

    #[test]
    fn draw_respects_the_modulus() {
        let mut rng = SplitMix64::new(11);
        for _ in 0..256 {
            assert!(rng.draw(93) < 93);
        }
        assert_eq!(rng.draw(0), 0);
    }
}
