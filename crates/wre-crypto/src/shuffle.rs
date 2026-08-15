use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

use wre_core::error::{Error, Result};

use crate::prng::Rng;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Alphabet {
    symbols: Vec<char>,
    index: BTreeMap<char, usize>,
}

impl Alphabet {
    pub fn new(symbols: impl IntoIterator<Item = char>) -> Result<Self> {
        let symbols: Vec<char> = symbols.into_iter().collect();
        if symbols.is_empty() {
            return Err(Error::msg("an alphabet needs at least one symbol"));
        }

        let mut index = BTreeMap::new();
        for (position, symbol) in symbols.iter().enumerate() {
            if index.insert(*symbol, position).is_some() {
                return Err(Error::msg(format!("symbol {symbol:?} appears twice")));
            }
        }

        Ok(Self { symbols, index })
    }

    pub fn parse(text: &str) -> Result<Self> {
        Self::new(text.chars())
    }

    pub fn printable_ascii(excluded: &str) -> Result<Self> {
        let excluded: Vec<char> = excluded.chars().collect();
        Self::new((32u8..=126).map(char::from).filter(|c| !excluded.contains(c)))
    }

    pub fn len(&self) -> usize {
        self.symbols.len()
    }

    pub fn is_empty(&self) -> bool {
        self.symbols.is_empty()
    }

    pub fn symbols(&self) -> &[char] {
        &self.symbols
    }

    pub fn position(&self, symbol: char) -> Option<usize> {
        self.index.get(&symbol).copied()
    }

    pub fn at(&self, position: usize) -> char {
        self.symbols[position % self.symbols.len()]
    }
}

pub fn substitute(text: &str, alphabet: &Alphabet, rng: &mut dyn Rng) -> String {
    rotate(text, alphabet, rng, true)
}

pub fn unsubstitute(text: &str, alphabet: &Alphabet, rng: &mut dyn Rng) -> String {
    rotate(text, alphabet, rng, false)
}

fn rotate(text: &str, alphabet: &Alphabet, rng: &mut dyn Rng, forward: bool) -> String {
    let width = alphabet.len();
    let mut out = String::with_capacity(text.len());

    for symbol in text.chars() {
        let step = (rng.next_u64() % width as u64) as usize;
        match alphabet.position(symbol) {
            Some(position) => {
                let moved = if forward {
                    (position + step) % width
                } else {
                    (position + width - step % width) % width
                };
                out.push(alphabet.at(moved));
            }
            None => out.push(symbol),
        }
    }

    out
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Permutation {
    map: Vec<usize>,
}

impl Permutation {
    pub fn identity(len: usize) -> Self {
        Self { map: (0..len).collect() }
    }

    pub fn new(map: Vec<usize>) -> Result<Self> {
        let mut seen = vec![false; map.len()];
        for &target in &map {
            if target >= map.len() {
                return Err(Error::msg(format!(
                    "permutation entry {target} is outside a length of {}",
                    map.len()
                )));
            }
            if seen[target] {
                return Err(Error::msg(format!("permutation repeats index {target}")));
            }
            seen[target] = true;
        }
        Ok(Self { map })
    }

    pub fn fisher_yates(len: usize, rng: &mut dyn Rng) -> Self {
        let mut map: Vec<usize> = (0..len).collect();
        if len < 2 {
            return Self { map };
        }

        for position in (1..len).rev() {
            let choice = (rng.next_u64() % (position as u64 + 1)) as usize;
            map.swap(position, choice);
        }

        Self { map }
    }

    pub fn len(&self) -> usize {
        self.map.len()
    }

    pub fn is_empty(&self) -> bool {
        self.map.is_empty()
    }

    pub fn map(&self) -> &[usize] {
        &self.map
    }

    pub fn invert(&self) -> Self {
        let mut map = vec![0usize; self.map.len()];
        for (position, &source) in self.map.iter().enumerate() {
            map[source] = position;
        }
        Self { map }
    }

    pub fn compose(&self, other: &Self) -> Result<Self> {
        if self.map.len() != other.map.len() {
            return Err(Error::msg("permutations of different lengths do not compose"));
        }
        Ok(Self { map: self.map.iter().map(|&index| other.map[index]).collect() })
    }

    pub fn power(&self, times: usize) -> Result<Self> {
        let mut out = Permutation::identity(self.map.len());
        for _ in 0..times {
            out = out.compose(self)?;
        }
        Ok(out)
    }

    pub fn cycles(&self) -> Vec<Vec<usize>> {
        let mut seen = vec![false; self.map.len()];
        let mut out = Vec::new();

        for start in 0..self.map.len() {
            if seen[start] {
                continue;
            }

            let mut cycle = Vec::new();
            let mut position = start;
            while !seen[position] {
                seen[position] = true;
                cycle.push(position);
                position = self.map[position];
            }

            if cycle.len() > 1 {
                out.push(cycle);
            }
        }

        out
    }

    pub fn order(&self) -> usize {
        self.cycles()
            .iter()
            .map(Vec::len)
            .fold(1usize, |acc, len| lcm(acc, len))
    }

    pub fn apply<T: Clone>(&self, values: &[T]) -> Result<Vec<T>> {
        if values.len() != self.map.len() {
            return Err(Error::msg(format!(
                "cannot permute {} values with a permutation of {}",
                values.len(),
                self.map.len()
            )));
        }

        let mut out: Vec<Option<T>> = vec![None; values.len()];
        for (position, value) in values.iter().enumerate() {
            out[self.map[position]] = Some(value.clone());
        }

        Ok(out.into_iter().flatten().collect())
    }

    pub fn unapply<T: Clone>(&self, values: &[T]) -> Result<Vec<T>> {
        self.invert().apply(values)
    }
}

fn lcm(left: usize, right: usize) -> usize {
    if left == 0 || right == 0 {
        return 0;
    }
    left / gcd(left, right) * right
}

fn gcd(mut left: usize, mut right: usize) -> usize {
    while right != 0 {
        let next = left % right;
        left = right;
        right = next;
    }
    left
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::prng::{Lcg, SplitMix64};

    fn lcg() -> Lcg {
        Lcg::new(1, 65_793, 4_282_663, 0x7f_ffff).output(8, 0xffff)
    }

    #[test]
    fn substitution_round_trips_with_a_replayed_stream() {
        let alphabet = Alphabet::printable_ascii("\"'\\").unwrap();
        assert_eq!(alphabet.len(), 92);

        let text = "{\"ver\":3,\"tst\":1700000000}";
        let sealed = substitute(text, &alphabet, &mut lcg());
        assert_ne!(sealed, text);
        assert_eq!(unsubstitute(&sealed, &alphabet, &mut lcg()), text);
    }

    #[test]
    fn symbols_outside_the_alphabet_pass_through_but_still_consume_a_draw() {
        let alphabet = Alphabet::parse("abc").unwrap();
        let sealed = substitute("a\u{263A}c", &alphabet, &mut lcg());
        assert!(sealed.contains('\u{263A}'));
        assert_eq!(unsubstitute(&sealed, &alphabet, &mut lcg()), "a\u{263A}c");
    }

    #[test]
    fn a_duplicate_symbol_is_rejected() {
        assert!(Alphabet::parse("aab").is_err());
    }

    #[test]
    fn a_permutation_inverts() {
        let mut rng = SplitMix64::new(4);
        let permutation = Permutation::fisher_yates(24, &mut rng);
        let values: Vec<usize> = (0..24).collect();

        let moved = permutation.apply(&values).unwrap();
        assert_ne!(moved, values);
        assert_eq!(permutation.unapply(&moved).unwrap(), values);
    }

    #[test]
    fn repeated_application_matches_a_power() {
        let mut rng = SplitMix64::new(9);
        let permutation = Permutation::fisher_yates(16, &mut rng);
        let values: Vec<usize> = (0..16).collect();

        let mut stepped = values.clone();
        for _ in 0..5 {
            stepped = permutation.apply(&stepped).unwrap();
        }

        assert_eq!(permutation.power(5).unwrap().apply(&values).unwrap(), stepped);
    }

    #[test]
    fn a_permutation_returns_to_identity_at_its_order() {
        let mut rng = SplitMix64::new(21);
        let permutation = Permutation::fisher_yates(12, &mut rng);
        let order = permutation.order();

        assert!(order >= 1);
        assert_eq!(permutation.power(order).unwrap(), Permutation::identity(12));
    }

    #[test]
    fn a_malformed_map_is_rejected() {
        assert!(Permutation::new(vec![0, 1, 1]).is_err());
        assert!(Permutation::new(vec![0, 1, 9]).is_err());
        assert!(Permutation::new(vec![2, 0, 1]).is_ok());
    }
}
