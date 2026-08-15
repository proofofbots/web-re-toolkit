use serde::{Deserialize, Serialize};

use wre_core::error::{Error, Result};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Continuation {
    #[default]
    HighContinues,
    LowContinues,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum DigitOrder {
    #[default]
    MostSignificantFirst,
    LeastSignificantFirst,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Radix {
    pub alphabet: Vec<char>,
    pub radix: u32,
    #[serde(default)]
    pub continuation: Continuation,
    #[serde(default)]
    pub order: DigitOrder,
}

impl Radix {
    pub fn new(alphabet: &str, radix: u32) -> Result<Self> {
        let symbols: Vec<char> = alphabet.chars().collect();

        if radix == 0 {
            return Err(Error::msg("the radix must be at least one"));
        }
        if symbols.len() < radix as usize * 2 {
            return Err(Error::msg(format!(
                "an alphabet of {} symbols cannot carry a continuation flag at radix {radix}, it needs {}",
                symbols.len(),
                radix * 2
            )));
        }

        let mut seen = symbols.clone();
        seen.sort_unstable();
        seen.dedup();
        if seen.len() != symbols.len() {
            return Err(Error::msg("the alphabet repeats a symbol"));
        }

        Ok(Self {
            alphabet: symbols,
            radix,
            continuation: Continuation::default(),
            order: DigitOrder::default(),
        })
    }

    pub fn with(mut self, continuation: Continuation, order: DigitOrder) -> Self {
        self.continuation = continuation;
        self.order = order;
        self
    }

    pub fn position(&self, symbol: char) -> Option<u32> {
        self.alphabet
            .iter()
            .position(|entry| *entry == symbol)
            .map(|index| index as u32)
    }

    fn split(&self, index: u32) -> (u32, bool) {
        let digit = index % self.radix;
        let high = index >= self.radix;

        let continues = match self.continuation {
            Continuation::HighContinues => high,
            Continuation::LowContinues => !high,
        };

        (digit, continues)
    }

    fn join(&self, digit: u32, continues: bool) -> u32 {
        let high = match self.continuation {
            Continuation::HighContinues => continues,
            Continuation::LowContinues => !continues,
        };

        if high { digit + self.radix } else { digit }
    }

    pub fn decode(&self, text: &str) -> Result<Vec<u64>> {
        let mut out = Vec::new();
        let mut digits: Vec<u32> = Vec::new();

        for symbol in text.chars() {
            let index = self
                .position(symbol)
                .ok_or_else(|| Error::msg(format!("symbol {symbol:?} is not in the alphabet")))?;

            let (digit, continues) = self.split(index);
            digits.push(digit);

            if !continues {
                out.push(self.assemble(&digits));
                digits.clear();
            }
        }

        if !digits.is_empty() {
            return Err(Error::msg("the stream ends inside an unterminated value"));
        }

        Ok(out)
    }

    fn assemble(&self, digits: &[u32]) -> u64 {
        let radix = u64::from(self.radix);

        match self.order {
            DigitOrder::MostSignificantFirst => digits
                .iter()
                .fold(0u64, |acc, digit| acc.saturating_mul(radix) + u64::from(*digit)),
            DigitOrder::LeastSignificantFirst => digits
                .iter()
                .rev()
                .fold(0u64, |acc, digit| acc.saturating_mul(radix) + u64::from(*digit)),
        }
    }

    pub fn encode(&self, values: &[u64]) -> Result<String> {
        let mut out = String::new();

        for value in values {
            let mut digits = Vec::new();
            let mut left = *value;
            let radix = u64::from(self.radix);

            loop {
                digits.push((left % radix) as u32);
                left /= radix;
                if left == 0 {
                    break;
                }
            }

            if self.order == DigitOrder::MostSignificantFirst {
                digits.reverse();
            }

            let last = digits.len() - 1;
            for (position, digit) in digits.iter().enumerate() {
                let index = self.join(*digit, position != last);
                let symbol = self.alphabet.get(index as usize).ok_or_else(|| {
                    Error::msg(format!("index {index} is outside the alphabet"))
                })?;
                out.push(*symbol);
            }
        }

        Ok(out)
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Shape {
    pub radix: u32,
    pub continuation: Continuation,
    pub order: DigitOrder,
}

pub fn fit(alphabet: &str, text: &str, expected: &[u64], max_radix: u32) -> Vec<Shape> {
    let mut out = Vec::new();
    let symbols = alphabet.chars().count() as u32;

    for radix in 1..=max_radix.min(symbols / 2) {
        for continuation in [Continuation::HighContinues, Continuation::LowContinues] {
            for order in [DigitOrder::MostSignificantFirst, DigitOrder::LeastSignificantFirst] {
                let Ok(codec) = Radix::new(alphabet, radix) else {
                    continue;
                };

                let codec = codec.with(continuation, order);
                let Ok(values) = codec.decode(text) else {
                    continue;
                };

                if values.len() >= expected.len() && values[..expected.len()] == *expected {
                    out.push(Shape { radix, continuation, order });
                }
            }
        }
    }

    out
}

#[cfg(test)]
mod tests {
    use super::*;

    const ALPHABET: &str = "abcdefghijklmnopqrstuvwxyz0123456789ABCDEFGHIJKLMNOPQRSTUVWXYZ+/@#$%";

    #[test]
    fn a_stream_round_trips() {
        let codec = Radix::new(ALPHABET, 20).unwrap();
        let values = vec![0, 1, 19, 20, 399, 400, 123_456];

        let text = codec.encode(&values).unwrap();
        assert_eq!(codec.decode(&text).unwrap(), values);
    }

    #[test]
    fn every_shape_round_trips() {
        let values = vec![7, 42, 1_000, 65_535];

        for continuation in [Continuation::HighContinues, Continuation::LowContinues] {
            for order in [DigitOrder::MostSignificantFirst, DigitOrder::LeastSignificantFirst] {
                let codec = Radix::new(ALPHABET, 16).unwrap().with(continuation, order);
                let text = codec.encode(&values).unwrap();
                assert_eq!(codec.decode(&text).unwrap(), values, "{continuation:?} {order:?}");
            }
        }
    }

    #[test]
    fn single_digit_values_use_one_symbol_each() {
        let codec = Radix::new(ALPHABET, 20).unwrap();
        assert_eq!(codec.encode(&[0, 1, 2]).unwrap().chars().count(), 3);
    }

    #[test]
    fn an_alphabet_too_small_for_the_radix_is_rejected() {
        assert!(Radix::new("abcd", 3).is_err());
        assert!(Radix::new("abcdef", 3).is_ok());
        assert!(Radix::new(ALPHABET, 0).is_err());
        assert!(Radix::new("aab", 1).is_err());
    }

    #[test]
    fn an_unknown_symbol_is_reported() {
        let codec = Radix::new(ALPHABET, 20).unwrap();
        let error = codec.decode("ab~").unwrap_err().to_string();
        assert!(error.contains("not in the alphabet"), "{error}");
    }

    #[test]
    fn an_unterminated_stream_is_reported() {
        let codec = Radix::new(ALPHABET, 20).unwrap();
        let text = codec.encode(&[123_456]).unwrap();
        let clipped: String = text.chars().take(text.chars().count() - 1).collect();
        assert!(codec.decode(&clipped).unwrap_err().to_string().contains("unterminated"));
    }

    #[test]
    fn fitting_recovers_the_shape_that_produced_a_stream() {
        let codec = Radix::new(ALPHABET, 12)
            .unwrap()
            .with(Continuation::LowContinues, DigitOrder::LeastSignificantFirst);

        let values = vec![5, 61, 4_000, 12];
        let text = codec.encode(&values).unwrap();

        let found = fit(ALPHABET, &text, &values, 34);
        assert!(found.contains(&Shape {
            radix: 12,
            continuation: Continuation::LowContinues,
            order: DigitOrder::LeastSignificantFirst,
        }));
    }

    #[test]
    fn fitting_reports_nothing_when_the_expectation_does_not_hold() {
        let codec = Radix::new(ALPHABET, 12).unwrap();
        let text = codec.encode(&[1, 2, 3]).unwrap();
        assert!(fit(ALPHABET, &text, &[9, 9, 9], 34).is_empty());
    }
}
