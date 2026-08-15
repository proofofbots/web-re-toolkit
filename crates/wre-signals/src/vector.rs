use std::collections::{BTreeMap, BTreeSet};

use serde::{Deserialize, Serialize};
use serde_json::Value;

use wre_core::error::{Error, Result};
use wre_crypto::shuffle::Permutation;

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct Alignment {
    pub pairs: BTreeMap<usize, usize>,
    pub ambiguous: Vec<usize>,
    pub unmatched: Vec<usize>,
    pub appeared: Vec<usize>,
}

impl Alignment {
    pub fn follow(&self, slot: usize) -> Option<usize> {
        self.pairs.get(&slot).copied()
    }

    pub fn coverage(&self, width: usize) -> f64 {
        if width == 0 {
            return 1.0;
        }
        self.pairs.len() as f64 / width as f64
    }

    pub fn shift_histogram(&self) -> BTreeMap<i64, usize> {
        let mut out = BTreeMap::new();
        for (before, after) in &self.pairs {
            let shift = *after as i64 - *before as i64;
            *out.entry(shift).or_insert(0) += 1;
        }
        out
    }

    pub fn dominant_shift(&self) -> Option<(i64, f64)> {
        let histogram = self.shift_histogram();
        let total: usize = histogram.values().sum();

        if total == 0 {
            return None;
        }

        histogram
            .into_iter()
            .max_by_key(|(_, count)| *count)
            .map(|(shift, count)| (shift, count as f64 / total as f64))
    }

    pub fn to_permutation(&self, width: usize) -> Result<Permutation> {
        if self.pairs.len() != width {
            return Err(Error::msg(format!(
                "only {} of {width} slots aligned, that is not a whole permutation",
                self.pairs.len()
            )));
        }

        let mut map = vec![0usize; width];
        for (before, after) in &self.pairs {
            if *before >= width || *after >= width {
                return Err(Error::msg("an aligned slot falls outside the vector"));
            }
            map[*before] = *after;
        }

        Permutation::new(map)
    }
}

fn signature(runs: &[Vec<Value>], slot: usize) -> String {
    let parts: Vec<String> = runs
        .iter()
        .map(|run| {
            run.get(slot)
                .map(|value| value.to_string())
                .unwrap_or_else(|| "<short>".to_string())
        })
        .collect();

    parts.join("\u{1f}")
}

fn check(runs: &[Vec<Value>], label: &str) -> Result<usize> {
    let first = runs
        .first()
        .ok_or_else(|| Error::msg(format!("the {label} build has no runs")))?;

    if runs.iter().any(|run| run.len() != first.len()) {
        return Err(Error::msg(format!(
            "the {label} build has runs of different widths"
        )));
    }

    Ok(first.len())
}

pub fn align(before: &[Vec<Value>], after: &[Vec<Value>]) -> Result<Alignment> {
    let before_width = check(before, "earlier")?;
    let after_width = check(after, "later")?;

    if before.len() != after.len() {
        return Err(Error::msg(format!(
            "alignment needs the same number of runs on both sides, got {} and {}",
            before.len(),
            after.len()
        )));
    }

    let mut before_index: BTreeMap<String, Vec<usize>> = BTreeMap::new();
    for slot in 0..before_width {
        before_index
            .entry(signature(before, slot))
            .or_default()
            .push(slot);
    }

    let mut after_index: BTreeMap<String, Vec<usize>> = BTreeMap::new();
    for slot in 0..after_width {
        after_index
            .entry(signature(after, slot))
            .or_default()
            .push(slot);
    }

    let mut alignment = Alignment::default();
    let mut taken: BTreeSet<usize> = BTreeSet::new();

    for slot in 0..before_width {
        let key = signature(before, slot);
        let mine = before_index.get(&key).map(Vec::as_slice).unwrap_or(&[]);
        let theirs = after_index.get(&key).map(Vec::as_slice).unwrap_or(&[]);

        match (mine.len(), theirs.len()) {
            (1, 1) => {
                alignment.pairs.insert(slot, theirs[0]);
                taken.insert(theirs[0]);
            }
            (_, 0) => alignment.unmatched.push(slot),
            _ => alignment.ambiguous.push(slot),
        }
    }

    alignment.appeared = (0..after_width).filter(|slot| !taken.contains(slot)).collect();

    Ok(alignment)
}

pub fn noise_slots(runs: &[Vec<Value>]) -> Result<BTreeSet<usize>> {
    let width = check(runs, "sampled")?;

    Ok((0..width)
        .filter(|slot| {
            let first = runs[0].get(*slot);
            runs.iter().any(|run| run.get(*slot) != first)
        })
        .collect())
}

pub fn stable_align(
    before: &[Vec<Value>],
    after: &[Vec<Value>],
) -> Result<(Alignment, BTreeSet<usize>)> {
    let noisy_before = noise_slots(before)?;
    let noisy_after = noise_slots(after)?;

    let mut alignment = align(before, after)?;

    alignment
        .pairs
        .retain(|slot, target| !noisy_before.contains(slot) && !noisy_after.contains(target));

    let mut noisy: BTreeSet<usize> = noisy_before;
    noisy.extend(noisy_after);

    Ok((alignment, noisy))
}

pub fn apply_rotation(values: &[Value], rotation: &Permutation, times: usize) -> Result<Vec<Value>> {
    rotation.power(times)?.apply(values)
}

pub fn recover_rotation(before: &[Value], after: &[Value]) -> Result<Permutation> {
    let alignment = align(&[before.to_vec()], &[after.to_vec()])?;
    alignment.to_permutation(before.len())
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn run(values: &[i64]) -> Vec<Value> {
        values.iter().map(|value| json!(value)).collect()
    }

    #[test]
    fn a_reordered_vector_is_aligned_from_one_run() {
        let before = vec![run(&[10, 20, 30, 40])];
        let after = vec![run(&[30, 10, 40, 20])];

        let alignment = align(&before, &after).unwrap();

        assert_eq!(alignment.follow(0), Some(1));
        assert_eq!(alignment.follow(1), Some(3));
        assert_eq!(alignment.follow(2), Some(0));
        assert_eq!(alignment.follow(3), Some(2));
        assert_eq!(alignment.coverage(4), 1.0);
    }

    #[test]
    fn repeated_values_are_reported_as_ambiguous_not_guessed() {
        let before = vec![run(&[0, 0, 7])];
        let after = vec![run(&[7, 0, 0])];

        let alignment = align(&before, &after).unwrap();

        assert_eq!(alignment.follow(2), Some(0));
        assert_eq!(alignment.ambiguous, vec![0, 1]);
        assert!(alignment.coverage(3) < 1.0);
    }

    #[test]
    fn several_runs_disambiguate_what_one_run_cannot() {
        let before = vec![run(&[0, 0, 7]), run(&[1, 2, 7])];
        let after = vec![run(&[7, 0, 0]), run(&[7, 2, 1])];

        let alignment = align(&before, &after).unwrap();

        assert!(alignment.ambiguous.is_empty());
        assert_eq!(alignment.follow(0), Some(2));
        assert_eq!(alignment.follow(1), Some(1));
        assert_eq!(alignment.follow(2), Some(0));
    }

    #[test]
    fn a_slot_with_no_counterpart_is_unmatched_and_new_slots_are_listed() {
        let before = vec![run(&[1, 2, 3])];
        let after = vec![run(&[3, 1, 99])];

        let alignment = align(&before, &after).unwrap();

        assert_eq!(alignment.unmatched, vec![1]);
        assert_eq!(alignment.appeared, vec![2]);
    }

    #[test]
    fn a_whole_alignment_becomes_a_permutation_that_replays() {
        let before = run(&[10, 20, 30, 40]);
        let after = run(&[30, 10, 40, 20]);

        let rotation = recover_rotation(&before, &after).unwrap();
        assert_eq!(rotation.apply(&before).unwrap(), after);

        let twice = apply_rotation(&before, &rotation, 2).unwrap();
        assert_eq!(rotation.apply(&after).unwrap(), twice);
    }

    #[test]
    fn a_partial_alignment_refuses_to_become_a_permutation() {
        let before = vec![run(&[1, 1, 3])];
        let after = vec![run(&[3, 1, 1])];

        let error = align(&before, &after)
            .unwrap()
            .to_permutation(3)
            .unwrap_err()
            .to_string();

        assert!(error.contains("not a whole permutation"), "{error}");
    }

    #[test]
    fn a_rotation_shows_up_as_one_dominant_shift() {
        let before = vec![run(&[1, 2, 3, 4, 5, 6, 7, 8])];
        let after = vec![run(&[8, 1, 2, 3, 4, 5, 6, 7])];

        let alignment = align(&before, &after).unwrap();
        let (shift, agreement) = alignment.dominant_shift().unwrap();

        assert_eq!(shift, 1);
        assert!(agreement > 0.8, "seven of eight slots shift by one, got {agreement}");
    }

    #[test]
    fn a_scrambled_vector_has_no_dominant_shift() {
        let before = vec![run(&[1, 2, 3, 4, 5, 6, 7, 8])];
        let after = vec![run(&[3, 8, 1, 6, 4, 2, 7, 5])];

        let alignment = align(&before, &after).unwrap();
        let (_, agreement) = alignment.dominant_shift().unwrap();

        assert!(agreement < 0.5, "agreement was {agreement}");
        assert!(alignment.shift_histogram().len() > 4);
    }

    #[test]
    fn an_empty_alignment_has_no_shift_at_all() {
        assert!(Alignment::default().dominant_shift().is_none());
    }

    #[test]
    fn slots_that_move_between_identical_runs_are_noise() {
        let runs = vec![run(&[1, 2, 3]), run(&[1, 9, 3])];
        assert_eq!(noise_slots(&runs).unwrap(), BTreeSet::from([1]));
    }

    #[test]
    fn noisy_slots_are_dropped_from_a_stable_alignment() {
        let before = vec![run(&[1, 2, 3]), run(&[1, 9, 3])];
        let after = vec![run(&[3, 2, 1]), run(&[3, 9, 1])];

        let (alignment, noisy) = stable_align(&before, &after).unwrap();

        assert!(noisy.contains(&1));
        assert!(!alignment.pairs.contains_key(&1));
        assert_eq!(alignment.follow(0), Some(2));
        assert_eq!(alignment.follow(2), Some(0));
    }

    #[test]
    fn mismatched_inputs_are_rejected() {
        assert!(align(&[], &[run(&[1])]).is_err());
        assert!(align(&[run(&[1])], &[run(&[1]), run(&[2])]).is_err());
        assert!(align(&[run(&[1]), run(&[1, 2])], &[run(&[1]), run(&[1])]).is_err());
    }
}
