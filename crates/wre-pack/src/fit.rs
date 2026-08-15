use serde::{Deserialize, Serialize};

use wre_core::error::{Error, Result};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Linear {
    pub base: i64,
    pub slope: i64,
    pub step: i64,
    pub digits: Vec<i64>,
}

impl Linear {
    pub fn value(&self, index: usize, digit: i64) -> i64 {
        self.base + self.slope * index as i64 + digit * self.step
    }

    pub fn rebuild(&self) -> Vec<i64> {
        self.digits
            .iter()
            .enumerate()
            .map(|(index, digit)| self.value(index, *digit))
            .collect()
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Bounds {
    pub slopes: Vec<i64>,
    pub steps: Vec<i64>,
    pub lowest_digit: i64,
    pub highest_digit: i64,
}

impl Default for Bounds {
    fn default() -> Self {
        Self {
            slopes: (-4..=4).collect(),
            steps: vec![1, 2, 4, 8, 16, 32, 64, 128, 256],
            lowest_digit: -8,
            highest_digit: 63,
        }
    }
}

pub fn fit_linear(values: &[i64], bounds: &Bounds) -> Result<Vec<Linear>> {
    if values.is_empty() {
        return Err(Error::msg("nothing to fit"));
    }
    if bounds.lowest_digit > bounds.highest_digit {
        return Err(Error::msg("the digit range is inverted"));
    }

    let mut out = Vec::new();

    for slope in &bounds.slopes {
        let residuals: Vec<i64> = values
            .iter()
            .enumerate()
            .map(|(index, value)| value - slope * index as i64)
            .collect();

        for step in &bounds.steps {
            if *step == 0 {
                continue;
            }

            let anchor = residuals.iter().copied().min().unwrap_or(0);
            if residuals.iter().any(|residual| (residual - anchor) % step != 0) {
                continue;
            }

            let base = anchor - bounds.lowest_digit * step;
            let digits: Vec<i64> = residuals
                .iter()
                .map(|residual| (residual - base) / step)
                .collect();

            if digits
                .iter()
                .any(|digit| *digit < bounds.lowest_digit || *digit > bounds.highest_digit)
            {
                continue;
            }

            let candidate = Linear { base, slope: *slope, step: *step, digits };
            if candidate.rebuild() == values {
                out.push(candidate);
            }
        }
    }

    out.sort_by_key(|candidate| (std::cmp::Reverse(candidate.step.abs()), candidate.slope.abs()));
    out.dedup();
    Ok(out)
}

pub fn is_linear(values: &[i64]) -> bool {
    if values.len() < 2 {
        return true;
    }
    let delta = values[1] - values[0];
    values.windows(2).all(|pair| pair[1] - pair[0] == delta)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn build(base: i64, slope: i64, step: i64, digits: &[i64]) -> Vec<i64> {
        digits
            .iter()
            .enumerate()
            .map(|(index, digit)| base + slope * index as i64 + digit * step)
            .collect()
    }

    #[test]
    fn a_known_encoding_is_recovered() {
        let digits = vec![3i64, 0, 7, 15, 2, 9];
        let values = build(1000, -1, 16, &digits);

        let found = fit_linear(&values, &Bounds::default()).unwrap();
        let best = found.first().expect("no fit found");

        assert_eq!(best.rebuild(), values);
        assert_eq!(best.slope, -1);
        assert_eq!(best.step, 16);
        assert!(
            found.iter().any(|fit| fit.step == 4),
            "a divisor of the true step also fits and should be reported"
        );
        assert_eq!(
            best.digits.iter().map(|digit| digit - best.digits[1]).collect::<Vec<_>>(),
            digits.iter().map(|digit| digit - digits[1]).collect::<Vec<_>>()
        );
    }

    #[test]
    fn a_flat_encoding_with_no_slope_is_recovered() {
        let digits = vec![0i64, 5, 2, 9, 1];
        let values = build(64, 0, 1, &digits);

        let found = fit_linear(&values, &Bounds::default()).unwrap();
        assert!(found.iter().any(|fit| fit.slope == 0 && fit.step == 1));
        assert!(found.iter().all(|fit| fit.rebuild() == values));
    }

    #[test]
    fn every_reported_fit_reproduces_the_input() {
        let values = build(500, 2, 8, &[1, 4, 4, 0, 7, 3]);
        for fit in fit_linear(&values, &Bounds::default()).unwrap() {
            assert_eq!(fit.rebuild(), values);
        }
    }

    #[test]
    fn a_sequence_outside_the_digit_range_does_not_fit() {
        let bounds = Bounds { lowest_digit: 0, highest_digit: 3, ..Bounds::default() };
        let values = build(0, 0, 1, &[0, 1, 2, 900]);
        assert!(fit_linear(&values, &bounds).unwrap().is_empty());
    }

    #[test]
    fn nothing_and_an_inverted_range_are_rejected() {
        assert!(fit_linear(&[], &Bounds::default()).is_err());

        let bounds = Bounds { lowest_digit: 10, highest_digit: 0, ..Bounds::default() };
        assert!(fit_linear(&[1, 2], &bounds).is_err());
    }

    #[test]
    fn a_plain_arithmetic_run_is_recognised() {
        assert!(is_linear(&[3, 6, 9, 12]));
        assert!(is_linear(&[7]));
        assert!(!is_linear(&[1, 2, 4]));
    }
}
