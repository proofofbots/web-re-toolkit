use std::time::{Duration, Instant};

use serde::{Deserialize, Serialize};

use wre_core::error::Result;

use crate::table::Table;

pub type CheckFn = Box<dyn FnMut() -> Result<String>>;

pub struct Check {
    pub name: String,
    pub note: String,
    pub run: CheckFn,
}

impl Check {
    pub fn new<F>(name: &str, note: &str, run: F) -> Self
    where
        F: FnMut() -> Result<String> + 'static,
    {
        Self {
            name: name.to_string(),
            note: note.to_string(),
            run: Box::new(run),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CheckOutcome {
    pub name: String,
    pub note: String,
    pub passed: bool,
    pub detail: String,
    pub millis: u128,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Report {
    pub outcomes: Vec<CheckOutcome>,
    pub millis: u128,
}

impl Report {
    pub fn passed(&self) -> usize {
        self.outcomes.iter().filter(|entry| entry.passed).count()
    }

    pub fn failed(&self) -> usize {
        self.outcomes.len() - self.passed()
    }

    pub fn ok(&self) -> bool {
        self.failed() == 0 && !self.outcomes.is_empty()
    }

    pub fn headline(&self) -> String {
        format!("{} of {} checks pass", self.passed(), self.outcomes.len())
    }

    pub fn failures(&self) -> Vec<&CheckOutcome> {
        self.outcomes.iter().filter(|entry| !entry.passed).collect()
    }

    pub fn render(&self) -> String {
        let mut table = Table::new(&["check", "result", "detail"]);

        for outcome in &self.outcomes {
            table.push(vec![
                outcome.name.clone(),
                if outcome.passed { "pass".into() } else { "fail".into() },
                outcome.detail.clone(),
            ]);
        }

        format!("{}\n{}\n", self.headline(), table.render())
    }
}

#[derive(Default)]
pub struct Acceptance {
    checks: Vec<Check>,
}

impl Acceptance {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn check<F>(mut self, name: &str, note: &str, run: F) -> Self
    where
        F: FnMut() -> Result<String> + 'static,
    {
        self.checks.push(Check::new(name, note, run));
        self
    }

    pub fn add(&mut self, check: Check) -> &mut Self {
        self.checks.push(check);
        self
    }

    pub fn len(&self) -> usize {
        self.checks.len()
    }

    pub fn is_empty(&self) -> bool {
        self.checks.is_empty()
    }

    pub fn run(mut self) -> Report {
        let started = Instant::now();
        let mut outcomes = Vec::with_capacity(self.checks.len());

        for check in &mut self.checks {
            let at = Instant::now();
            let outcome = match (check.run)() {
                Ok(detail) => CheckOutcome {
                    name: check.name.clone(),
                    note: check.note.clone(),
                    passed: true,
                    detail,
                    millis: at.elapsed().as_millis(),
                },
                Err(error) => CheckOutcome {
                    name: check.name.clone(),
                    note: check.note.clone(),
                    passed: false,
                    detail: error.to_string(),
                    millis: at.elapsed().as_millis(),
                },
            };
            outcomes.push(outcome);
        }

        Report {
            outcomes,
            millis: started.elapsed().as_millis(),
        }
    }

    pub fn run_with_budget(self, budget: Duration) -> Report {
        let started = Instant::now();
        let mut report = self.run();

        if started.elapsed() > budget {
            report.outcomes.push(CheckOutcome {
                name: "budget".to_string(),
                note: "the suite ran longer than its budget".to_string(),
                passed: false,
                detail: format!(
                    "took {}ms against a {}ms budget",
                    started.elapsed().as_millis(),
                    budget.as_millis()
                ),
                millis: 0,
            });
        }

        report
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use wre_core::error::Error;

    #[test]
    fn runs_every_check_and_counts_results() {
        let report = Acceptance::new()
            .check("cipher round trip", "encrypt then decrypt", || {
                Ok("48 bytes".to_string())
            })
            .check("checksum", "recompute the payload checksum", || {
                Ok("matches".to_string())
            })
            .check("key derivation", "derive statically", || {
                Err(Error::msg("no key material in this build"))
            })
            .run();

        assert_eq!(report.outcomes.len(), 3);
        assert_eq!(report.passed(), 2);
        assert_eq!(report.failed(), 1);
        assert!(!report.ok());
        assert_eq!(report.headline(), "2 of 3 checks pass");
        assert_eq!(report.failures()[0].name, "key derivation");
    }

    #[test]
    fn a_clean_suite_reports_ok() {
        let report = Acceptance::new()
            .check("only check", "", || Ok("fine".to_string()))
            .run();

        assert!(report.ok());
        assert!(report.render().contains("| only check | pass | fine |"));
    }

    #[test]
    fn an_empty_suite_is_not_ok() {
        assert!(!Acceptance::new().run().ok());
    }

    #[test]
    fn a_slow_suite_fails_its_budget() {
        let report = Acceptance::new()
            .check("slow", "", || {
                std::thread::sleep(Duration::from_millis(30));
                Ok("done".to_string())
            })
            .run_with_budget(Duration::from_millis(1));

        assert!(report.failures().iter().any(|entry| entry.name == "budget"));
    }
}
