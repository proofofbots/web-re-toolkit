use std::collections::{BTreeMap, BTreeSet};

use serde::{Deserialize, Serialize};

use wre_core::error::{Error, Result};

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct Design {
    pub markers: Vec<String>,
    pub pools: Vec<Vec<String>>,
}

impl Design {
    pub fn binary(markers: &[String]) -> Result<Self> {
        let mut unique = markers.to_vec();
        unique.sort();
        unique.dedup();

        if unique.len() != markers.len() {
            return Err(Error::msg("a design cannot list the same marker twice"));
        }
        if markers.is_empty() {
            return Err(Error::msg("a design needs at least one marker"));
        }

        let width = u32::BITS - (markers.len() as u32).leading_zeros();
        let pools = (0..width)
            .map(|bit| {
                markers
                    .iter()
                    .enumerate()
                    .filter(|(index, _)| ((index + 1) >> bit) & 1 == 1)
                    .map(|(_, name)| name.clone())
                    .collect()
            })
            .collect();

        Ok(Self { markers: markers.to_vec(), pools })
    }

    pub fn one_at_a_time(markers: &[String]) -> Result<Self> {
        if markers.is_empty() {
            return Err(Error::msg("a design needs at least one marker"));
        }

        Ok(Self {
            markers: markers.to_vec(),
            pools: markers.iter().map(|name| vec![name.clone()]).collect(),
        })
    }

    pub fn runs(&self) -> usize {
        self.pools.len()
    }

    pub fn membership(&self, marker: &str) -> Vec<bool> {
        self.pools
            .iter()
            .map(|pool| pool.iter().any(|name| name == marker))
            .collect()
    }

    pub fn is_separating(&self) -> bool {
        let mut seen: BTreeSet<Vec<bool>> = BTreeSet::new();

        for marker in &self.markers {
            let pattern = self.membership(marker);
            if pattern.iter().all(|member| !member) {
                return false;
            }
            if !seen.insert(pattern) {
                return false;
            }
        }

        true
    }
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct Observation {
    pub pool: usize,
    pub moved: BTreeSet<String>,
}

impl Observation {
    pub fn new(pool: usize, moved: impl IntoIterator<Item = String>) -> Self {
        Self { pool, moved: moved.into_iter().collect() }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "verdict", rename_all = "kebab-case")]
pub enum Cause {
    Marker { name: String, confirmed: bool },
    NoSingleMarker { pattern: Vec<bool> },
    Noise,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Finding {
    pub address: String,
    #[serde(flatten)]
    pub cause: Cause,
}

impl Finding {
    pub fn marker(&self) -> Option<&str> {
        match &self.cause {
            Cause::Marker { name, .. } => Some(name.as_str()),
            _ => None,
        }
    }

    pub fn is_confirmed(&self) -> bool {
        matches!(self.cause, Cause::Marker { confirmed: true, .. })
    }

    pub fn describe(&self) -> String {
        match &self.cause {
            Cause::Marker { name, confirmed } => {
                if *confirmed {
                    format!("{} moves when {name} is planted, confirmed on its own", self.address)
                } else {
                    format!(
                        "{} points at {name}, but two markers together can produce the same pattern, so confirm it alone",
                        self.address
                    )
                }
            }
            Cause::NoSingleMarker { pattern } => format!(
                "{} moves in the pools {:?}, which no single marker explains",
                self.address,
                pattern
                    .iter()
                    .enumerate()
                    .filter(|(_, moved)| **moved)
                    .map(|(pool, _)| pool)
                    .collect::<Vec<_>>()
            ),
            Cause::Noise => format!("{} moves between identical runs, so it carries no signal", self.address),
        }
    }
}

pub fn attribute_pools(
    design: &Design,
    observations: &[Observation],
    noise: &BTreeSet<String>,
) -> Result<Vec<Finding>> {
    if observations.len() != design.runs() {
        return Err(Error::msg(format!(
            "the design has {} pools but {} were observed",
            design.runs(),
            observations.len()
        )));
    }

    for observation in observations {
        if observation.pool >= design.runs() {
            return Err(Error::msg(format!(
                "observation names pool {} which the design does not have",
                observation.pool
            )));
        }
    }

    let mut ordered = vec![BTreeSet::new(); design.runs()];
    for observation in observations {
        ordered[observation.pool] = observation.moved.clone();
    }

    let mut addresses: BTreeSet<String> = BTreeSet::new();
    for moved in &ordered {
        addresses.extend(moved.iter().cloned());
    }

    let patterns: BTreeMap<Vec<bool>, String> = design
        .markers
        .iter()
        .map(|marker| (design.membership(marker), marker.clone()))
        .collect();

    Ok(addresses
        .into_iter()
        .map(|address| {
            if noise.contains(&address) {
                return Finding { address, cause: Cause::Noise };
            }

            let pattern: Vec<bool> = ordered.iter().map(|moved| moved.contains(&address)).collect();

            let cause = match patterns.get(&pattern) {
                Some(name) => Cause::Marker { name: name.clone(), confirmed: false },
                None => Cause::NoSingleMarker { pattern },
            };

            Finding { address, cause }
        })
        .collect())
}

pub fn to_confirm(findings: &[Finding]) -> Vec<String> {
    let mut out: Vec<String> = findings
        .iter()
        .filter(|finding| !finding.is_confirmed())
        .filter_map(|finding| finding.marker().map(str::to_string))
        .collect();

    out.sort();
    out.dedup();
    out
}

pub fn confirm(
    findings: &[Finding],
    alone: &BTreeMap<String, BTreeSet<String>>,
) -> Vec<Finding> {
    findings
        .iter()
        .map(|finding| {
            let Cause::Marker { name, .. } = &finding.cause else {
                return finding.clone();
            };

            match alone.get(name) {
                Some(moved) if moved.contains(&finding.address) => Finding {
                    address: finding.address.clone(),
                    cause: Cause::Marker { name: name.clone(), confirmed: true },
                },
                Some(_) => Finding {
                    address: finding.address.clone(),
                    cause: Cause::NoSingleMarker { pattern: Vec::new() },
                },
                None => finding.clone(),
            }
        })
        .collect()
}

pub fn render_pools(findings: &[Finding]) -> String {
    let mut out = String::from("| address | cause | confirmed |\n| --- | --- | --- |\n");

    for finding in findings {
        let (cause, confirmed) = match &finding.cause {
            Cause::Marker { name, confirmed } => {
                (name.clone(), if *confirmed { "yes" } else { "not yet" })
            }
            Cause::NoSingleMarker { .. } => ("no single marker".to_string(), "n/a"),
            Cause::Noise => ("noise".to_string(), "n/a"),
        };
        out.push_str(&format!("| {} | {cause} | {confirmed} |\n", finding.address));
    }

    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn markers(count: usize) -> Vec<String> {
        (0..count).map(|index| format!("marker-{index}")).collect()
    }

    #[test]
    fn a_binary_design_separates_every_marker() {
        for count in [1usize, 2, 3, 7, 8, 15, 16, 60] {
            let design = Design::binary(&markers(count)).unwrap();
            assert!(design.is_separating(), "{count} markers did not separate");
        }
    }

    #[test]
    fn a_binary_design_needs_far_fewer_runs_than_one_at_a_time() {
        let names = markers(60);

        assert_eq!(Design::one_at_a_time(&names).unwrap().runs(), 60);
        assert_eq!(Design::binary(&names).unwrap().runs(), 6);
    }

    #[test]
    fn a_marker_is_recovered_from_its_pool_pattern() {
        let names = markers(7);
        let design = Design::binary(&names).unwrap();

        let culprit = "marker-4";
        let membership = design.membership(culprit);

        let observations: Vec<Observation> = (0..design.runs())
            .map(|pool| {
                if membership[pool] {
                    Observation::new(pool, ["payload.slot17".to_string()])
                } else {
                    Observation::new(pool, [])
                }
            })
            .collect();

        let findings = attribute_pools(&design, &observations, &BTreeSet::new()).unwrap();

        assert_eq!(findings.len(), 1);
        assert_eq!(findings[0].marker(), Some(culprit));
        assert!(findings[0].describe().contains(culprit));
    }

    #[test]
    fn every_marker_in_a_design_is_separately_recoverable() {
        let names = markers(12);
        let design = Design::binary(&names).unwrap();

        for culprit in &names {
            let membership = design.membership(culprit);
            let address = format!("slot.{culprit}");

            let observations: Vec<Observation> = (0..design.runs())
                .map(|pool| {
                    if membership[pool] {
                        Observation::new(pool, [address.clone()])
                    } else {
                        Observation::new(pool, [])
                    }
                })
                .collect();

            let findings = attribute_pools(&design, &observations, &BTreeSet::new()).unwrap();
            assert_eq!(findings[0].marker(), Some(culprit.as_str()));
        }
    }

    #[test]
    fn a_pooled_verdict_starts_out_unconfirmed() {
        let design = Design::binary(&markers(7)).unwrap();
        let membership = design.membership("marker-4");

        let observations: Vec<Observation> = (0..design.runs())
            .map(|pool| {
                if membership[pool] {
                    Observation::new(pool, ["slot.1".to_string()])
                } else {
                    Observation::new(pool, [])
                }
            })
            .collect();

        let findings = attribute_pools(&design, &observations, &BTreeSet::new()).unwrap();

        assert_eq!(findings[0].marker(), Some("marker-4"));
        assert!(!findings[0].is_confirmed());
        assert!(findings[0].describe().contains("confirm it alone"));
        assert_eq!(to_confirm(&findings), vec!["marker-4".to_string()]);
    }

    #[test]
    fn two_markers_together_can_alias_onto_a_third_and_confirmation_catches_it() {
        let names = markers(7);
        let design = Design::binary(&names).unwrap();

        let first = design.membership("marker-1");
        let second = design.membership("marker-2");

        let observations: Vec<Observation> = (0..design.runs())
            .map(|pool| {
                if first[pool] || second[pool] {
                    Observation::new(pool, ["payload.shared".to_string()])
                } else {
                    Observation::new(pool, [])
                }
            })
            .collect();

        let pooled = attribute_pools(&design, &observations, &BTreeSet::new()).unwrap();
        let blamed = pooled[0].marker().expect("the pooled pass names one marker").to_string();
        assert!(!pooled[0].is_confirmed());

        let alone = BTreeMap::from([(blamed, BTreeSet::new())]);
        let confirmed = confirm(&pooled, &alone);

        assert!(confirmed[0].marker().is_none(), "planting it alone moved nothing");
    }

    #[test]
    fn a_real_single_cause_survives_confirmation() {
        let design = Design::binary(&markers(7)).unwrap();
        let membership = design.membership("marker-4");

        let observations: Vec<Observation> = (0..design.runs())
            .map(|pool| {
                if membership[pool] {
                    Observation::new(pool, ["slot.1".to_string()])
                } else {
                    Observation::new(pool, [])
                }
            })
            .collect();

        let pooled = attribute_pools(&design, &observations, &BTreeSet::new()).unwrap();
        let alone = BTreeMap::from([(
            "marker-4".to_string(),
            BTreeSet::from(["slot.1".to_string()]),
        )]);

        let confirmed = confirm(&pooled, &alone);

        assert_eq!(confirmed[0].marker(), Some("marker-4"));
        assert!(confirmed[0].is_confirmed());
        assert!(confirmed[0].describe().contains("confirmed on its own"));
        assert!(to_confirm(&confirmed).is_empty());
    }

    #[test]
    fn confirmation_leaves_a_marker_it_was_not_given_alone() {
        let design = Design::binary(&markers(3)).unwrap();
        let membership = design.membership("marker-0");

        let observations: Vec<Observation> = (0..design.runs())
            .map(|pool| {
                if membership[pool] {
                    Observation::new(pool, ["slot.1".to_string()])
                } else {
                    Observation::new(pool, [])
                }
            })
            .collect();

        let pooled = attribute_pools(&design, &observations, &BTreeSet::new()).unwrap();
        let confirmed = confirm(&pooled, &BTreeMap::new());

        assert_eq!(confirmed, pooled);
    }

    #[test]
    fn a_known_noisy_address_is_never_blamed_on_a_marker() {
        let design = Design::binary(&markers(3)).unwrap();
        let noise = BTreeSet::from(["payload.timing".to_string()]);

        let observations: Vec<Observation> = (0..design.runs())
            .map(|pool| Observation::new(pool, ["payload.timing".to_string()]))
            .collect();

        let findings = attribute_pools(&design, &observations, &noise).unwrap();

        assert_eq!(findings[0].cause, Cause::Noise);
        assert!(findings[0].describe().contains("no signal"));
    }

    #[test]
    fn an_address_that_never_moves_is_never_reported() {
        let design = Design::binary(&markers(3)).unwrap();
        let observations: Vec<Observation> =
            (0..design.runs()).map(|pool| Observation::new(pool, [])).collect();

        assert!(attribute_pools(&design, &observations, &BTreeSet::new()).unwrap().is_empty());
    }

    #[test]
    fn a_mismatched_observation_count_is_rejected() {
        let design = Design::binary(&markers(7)).unwrap();
        let error = attribute_pools(&design, &[Observation::new(0, [])], &BTreeSet::new())
            .unwrap_err()
            .to_string();

        assert!(error.contains("3 pools but 1 were observed"), "{error}");
    }

    #[test]
    fn an_observation_naming_an_unknown_pool_is_rejected() {
        let design = Design::binary(&markers(3)).unwrap();
        let observations: Vec<Observation> = (0..design.runs())
            .map(|pool| Observation::new(pool + 10, []))
            .collect();

        assert!(attribute_pools(&design, &observations, &BTreeSet::new()).is_err());
    }

    #[test]
    fn a_design_rejects_duplicates_and_emptiness() {
        assert!(Design::binary(&[]).is_err());
        assert!(Design::one_at_a_time(&[]).is_err());
        assert!(Design::binary(&["a".to_string(), "a".to_string()]).is_err());
    }

    #[test]
    fn findings_render_as_a_table() {
        let design = Design::binary(&markers(3)).unwrap();
        let membership = design.membership("marker-0");

        let observations: Vec<Observation> = (0..design.runs())
            .map(|pool| {
                if membership[pool] {
                    Observation::new(pool, ["slot.1".to_string()])
                } else {
                    Observation::new(pool, [])
                }
            })
            .collect();

        let table = render_pools(&attribute_pools(&design, &observations, &BTreeSet::new()).unwrap());

        assert!(table.starts_with("| address | cause | confirmed |"));
        assert!(table.contains("| slot.1 | marker-0 | not yet |"));
    }
}
