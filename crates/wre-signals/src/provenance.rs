use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};
use serde_json::Value;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Access {
    pub api: String,
    pub value: Value,
    pub at: u64,
}

impl Access {
    pub fn new(api: impl Into<String>, value: Value, at: u64) -> Self {
        Self { api: api.into(), value, at }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Origin {
    Read,
    Derived,
    Computed,
    Constant,
    Unknown,
}

impl Origin {
    pub fn describe(self) -> &'static str {
        match self {
            Origin::Read => "read straight off the api",
            Origin::Derived => "a substring or cast of an api value",
            Origin::Computed => "built from api values that were read just before it",
            Origin::Constant => "the same in every run, so it comes from the build",
            Origin::Unknown => "no api was touched near it",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Attribution {
    pub address: String,
    pub origin: Origin,
    pub apis: Vec<String>,
    pub confidence: f64,
}

impl Attribution {
    pub fn best_api(&self) -> Option<&str> {
        self.apis.first().map(String::as_str)
    }
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Trace {
    pub accesses: Vec<Access>,
}

impl Trace {
    pub fn new(accesses: Vec<Access>) -> Self {
        let mut accesses = accesses;
        accesses.sort_by_key(|access| access.at);
        Self { accesses }
    }

    pub fn apis(&self) -> Vec<String> {
        let mut out: Vec<String> = self.accesses.iter().map(|entry| entry.api.clone()).collect();
        out.sort();
        out.dedup();
        out
    }

    pub fn window(&self, at: u64, back: u64) -> Vec<&Access> {
        self.accesses
            .iter()
            .filter(|access| access.at <= at && at.saturating_sub(access.at) <= back)
            .collect()
    }

    pub fn attribute(&self, address: &str, value: &Value, at: u64, back: u64) -> Attribution {
        let nearby = self.window(at, back);

        let exact: Vec<&&Access> = nearby.iter().filter(|access| &access.value == value).collect();
        if !exact.is_empty() {
            return Attribution {
                address: address.to_string(),
                origin: Origin::Read,
                apis: names(&exact),
                confidence: 1.0,
            };
        }

        let derived: Vec<&&Access> = nearby
            .iter()
            .filter(|access| is_derived(&access.value, value))
            .collect();
        if !derived.is_empty() {
            return Attribution {
                address: address.to_string(),
                origin: Origin::Derived,
                apis: names(&derived),
                confidence: 0.7,
            };
        }

        if nearby.is_empty() {
            return Attribution {
                address: address.to_string(),
                origin: Origin::Unknown,
                apis: Vec::new(),
                confidence: 0.0,
            };
        }

        let apis: Vec<&&Access> = nearby.iter().collect();
        Attribution {
            address: address.to_string(),
            origin: Origin::Computed,
            apis: names(&apis),
            confidence: (1.0 / nearby.len() as f64).min(0.5),
        }
    }
}

fn names(accesses: &[&&Access]) -> Vec<String> {
    let mut out: Vec<String> = accesses.iter().map(|access| access.api.clone()).collect();
    out.sort();
    out.dedup();
    out
}

fn is_derived(source: &Value, wanted: &Value) -> bool {
    match (source, wanted) {
        (Value::String(left), Value::String(right)) => {
            !right.is_empty() && left != right && left.contains(right.as_str())
        }
        (Value::String(left), Value::Number(right)) => left.parse::<f64>().ok() == right.as_f64(),
        (Value::Number(left), Value::String(right)) => left.to_string() == *right,
        (Value::Number(left), Value::Number(right)) => {
            left.as_f64().is_some_and(|value| {
                right
                    .as_f64()
                    .is_some_and(|other| other != value && value != 0.0 && (other / value).fract() == 0.0)
            })
        }
        (Value::Array(items), wanted) => items.contains(wanted),
        _ => false,
    }
}

pub fn constants(runs: &[BTreeMap<String, Value>]) -> Vec<String> {
    let Some(first) = runs.first() else {
        return Vec::new();
    };

    let mut out: Vec<String> = first
        .keys()
        .filter(|address| {
            let value = first.get(*address);
            runs.iter().all(|run| run.get(*address) == value)
        })
        .cloned()
        .collect();

    out.sort();
    out
}

pub fn attribute_all(
    trace: &Trace,
    fields: &BTreeMap<String, (Value, u64)>,
    constant_addresses: &[String],
    back: u64,
) -> Vec<Attribution> {
    let mut out: Vec<Attribution> = fields
        .iter()
        .map(|(address, (value, at))| {
            if constant_addresses.contains(address) {
                return Attribution {
                    address: address.clone(),
                    origin: Origin::Constant,
                    apis: Vec::new(),
                    confidence: 1.0,
                };
            }

            trace.attribute(address, value, *at, back)
        })
        .collect();

    out.sort_by(|left, right| left.address.cmp(&right.address));
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn trace() -> Trace {
        Trace::new(vec![
            Access::new("Navigator.userAgent", json!("Mozilla/5.0 Chrome/140"), 100),
            Access::new("Screen.width", json!(2560), 110),
            Access::new("Screen.height", json!(1440), 111),
            Access::new("Navigator.hardwareConcurrency", json!(8), 120),
        ])
    }

    #[test]
    fn a_value_copied_from_an_api_is_a_read() {
        let found = trace().attribute("payload.ua", &json!("Mozilla/5.0 Chrome/140"), 130, 50);

        assert_eq!(found.origin, Origin::Read);
        assert_eq!(found.best_api(), Some("Navigator.userAgent"));
        assert_eq!(found.confidence, 1.0);
    }

    #[test]
    fn a_substring_of_an_api_value_is_derived() {
        let found = trace().attribute("payload.browser", &json!("Chrome/140"), 130, 50);

        assert_eq!(found.origin, Origin::Derived);
        assert_eq!(found.best_api(), Some("Navigator.userAgent"));
    }

    #[test]
    fn a_number_that_matches_no_reading_falls_back_to_computed() {
        let found = trace().attribute("payload.mixed", &json!(2560 * 1440 + 7), 130, 50);

        assert_eq!(found.origin, Origin::Computed);
        assert!(found.apis.len() > 1);
        assert!(found.confidence < 0.5);
    }

    #[test]
    fn a_value_with_no_api_nearby_is_unknown() {
        let found = trace().attribute("payload.orphan", &json!("nothing"), 1_000, 10);

        assert_eq!(found.origin, Origin::Unknown);
        assert!(found.apis.is_empty());
        assert_eq!(found.confidence, 0.0);
    }

    #[test]
    fn the_window_only_looks_backwards() {
        let trace = trace();

        assert!(trace.window(105, 10).iter().any(|access| access.api == "Navigator.userAgent"));
        assert!(trace.window(99, 10).is_empty());
        assert_eq!(trace.window(130, 50).len(), 4);
    }

    #[test]
    fn every_origin_explains_itself() {
        for origin in [
            Origin::Read,
            Origin::Derived,
            Origin::Computed,
            Origin::Constant,
            Origin::Unknown,
        ] {
            assert!(!origin.describe().is_empty());
        }
    }

    #[test]
    fn addresses_that_never_move_are_constants_of_the_build() {
        let runs = vec![
            BTreeMap::from([
                ("build".to_string(), json!("a91f")),
                ("time".to_string(), json!(1)),
            ]),
            BTreeMap::from([
                ("build".to_string(), json!("a91f")),
                ("time".to_string(), json!(2)),
            ]),
        ];

        assert_eq!(constants(&runs), vec!["build".to_string()]);
        assert!(constants(&[]).is_empty());
    }

    #[test]
    fn a_known_constant_short_circuits_attribution() {
        let fields = BTreeMap::from([
            ("build".to_string(), (json!("a91f"), 130)),
            ("ua".to_string(), (json!("Mozilla/5.0 Chrome/140"), 130)),
        ]);

        let found = attribute_all(&trace(), &fields, &["build".to_string()], 50);

        assert_eq!(found[0].address, "build");
        assert_eq!(found[0].origin, Origin::Constant);
        assert_eq!(found[1].origin, Origin::Read);
    }

    #[test]
    fn the_trace_lists_the_apis_it_saw() {
        assert_eq!(
            trace().apis(),
            vec![
                "Navigator.hardwareConcurrency".to_string(),
                "Navigator.userAgent".to_string(),
                "Screen.height".to_string(),
                "Screen.width".to_string(),
            ]
        );
    }
}
