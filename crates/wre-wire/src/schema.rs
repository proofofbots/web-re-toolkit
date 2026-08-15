use std::collections::{BTreeMap, BTreeSet};

use serde::{Deserialize, Serialize};
use serde_json::Value;

use wre_core::address::{Address, leaves};

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum LeafShape {
    Null,
    Bool,
    Integer,
    Float,
    Text,
    EmptyArray,
    EmptyObject,
}

pub fn shape_of(value: &Value) -> LeafShape {
    match value {
        Value::Null => LeafShape::Null,
        Value::Bool(_) => LeafShape::Bool,
        Value::Number(number) => {
            if number.is_i64() || number.is_u64() {
                LeafShape::Integer
            } else {
                LeafShape::Float
            }
        }
        Value::String(_) => LeafShape::Text,
        Value::Array(_) => LeafShape::EmptyArray,
        Value::Object(_) => LeafShape::EmptyObject,
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Volatility {
    Constant,
    Uuid,
    Clock,
    Counter,
    Digest,
    Measured,
    Varying,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FieldSchema {
    pub address: Address,
    pub shapes: BTreeSet<LeafShape>,
    pub volatility: Volatility,
    pub present_in: usize,
    pub distinct: usize,
    #[serde(default)]
    pub samples: Vec<Value>,
    #[serde(default)]
    pub min: Option<f64>,
    #[serde(default)]
    pub max: Option<f64>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Schema {
    pub samples: usize,
    pub fields: Vec<FieldSchema>,
}

impl Schema {
    pub fn field(&self, address: &Address) -> Option<&FieldSchema> {
        self.fields.iter().find(|field| &field.address == address)
    }

    pub fn stable(&self) -> Vec<&FieldSchema> {
        self.fields
            .iter()
            .filter(|field| field.volatility == Volatility::Constant)
            .collect()
    }

    pub fn volatile(&self) -> Vec<&FieldSchema> {
        self.fields
            .iter()
            .filter(|field| field.volatility != Volatility::Constant)
            .collect()
    }

    pub fn always_present(&self) -> Vec<&FieldSchema> {
        self.fields
            .iter()
            .filter(|field| field.present_in == self.samples)
            .collect()
    }
}

pub fn infer(samples: &[Value]) -> Schema {
    let mut collected: BTreeMap<Address, Vec<Value>> = BTreeMap::new();

    for sample in samples {
        for (address, value) in leaves(sample) {
            collected.entry(address).or_default().push(value.clone());
        }
    }

    let mut fields = Vec::with_capacity(collected.len());

    for (address, values) in collected {
        let shapes: BTreeSet<LeafShape> = values.iter().map(shape_of).collect();

        let mut rendered: Vec<String> = values.iter().map(|value| value.to_string()).collect();
        rendered.sort();
        rendered.dedup();
        let distinct = rendered.len();

        let numbers: Vec<f64> = values.iter().filter_map(Value::as_f64).collect();
        let min = numbers.iter().copied().fold(None, |acc: Option<f64>, value| {
            Some(acc.map_or(value, |current| current.min(value)))
        });
        let max = numbers.iter().copied().fold(None, |acc: Option<f64>, value| {
            Some(acc.map_or(value, |current| current.max(value)))
        });

        let volatility = classify(&values, distinct, samples.len());

        let mut seen = BTreeSet::new();
        let mut sample_values = Vec::new();
        for value in &values {
            let key = value.to_string();
            if seen.insert(key) {
                sample_values.push(value.clone());
            }
            if sample_values.len() >= 3 {
                break;
            }
        }

        fields.push(FieldSchema {
            address,
            shapes,
            volatility,
            present_in: values.len(),
            distinct,
            samples: sample_values,
            min,
            max,
        });
    }

    Schema { samples: samples.len(), fields }
}

fn classify(values: &[Value], distinct: usize, samples: usize) -> Volatility {
    if distinct <= 1 {
        return Volatility::Constant;
    }

    let texts: Vec<&str> = values.iter().filter_map(Value::as_str).collect();

    if !texts.is_empty() && texts.len() == values.len() {
        if texts.iter().all(|text| looks_like_uuid(text)) {
            return Volatility::Uuid;
        }
        if texts.iter().all(|text| looks_like_digest(text)) {
            return Volatility::Digest;
        }
    }

    let numbers: Vec<f64> = values.iter().filter_map(Value::as_f64).collect();

    if numbers.len() == values.len() && numbers.len() > 1 {
        if numbers.iter().all(|value| looks_like_epoch(*value)) {
            return Volatility::Clock;
        }

        let monotonic = numbers.windows(2).all(|pair| pair[1] >= pair[0]);
        if monotonic && distinct == values.len() {
            return Volatility::Counter;
        }

        return Volatility::Measured;
    }

    if distinct == samples && samples > 1 {
        return Volatility::Varying;
    }

    Volatility::Varying
}

fn looks_like_uuid(text: &str) -> bool {
    let bytes = text.as_bytes();
    if bytes.len() != 36 {
        return false;
    }
    bytes.iter().enumerate().all(|(index, byte)| match index {
        8 | 13 | 18 | 23 => *byte == b'-',
        _ => byte.is_ascii_hexdigit(),
    })
}

fn looks_like_digest(text: &str) -> bool {
    matches!(text.len(), 8 | 16 | 32 | 40 | 64)
        && text.bytes().all(|byte| byte.is_ascii_hexdigit())
}

fn looks_like_epoch(value: f64) -> bool {
    (1_000_000_000.0..=4_000_000_000.0).contains(&value)
        || (1_000_000_000_000.0..=4_000_000_000_000.0).contains(&value)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn classifies_leaf_volatility() {
        let samples = vec![
            json!({
                "fixed": "same",
                "id": "3f2504e0-4f89-41d3-9a0c-0305e82c3301",
                "clock": 1700000000000i64,
                "count": 1,
                "digest": "deadbeefdeadbeef"
            }),
            json!({
                "fixed": "same",
                "id": "3f2504e0-4f89-41d3-9a0c-0305e82c3302",
                "clock": 1700000005000i64,
                "count": 2,
                "digest": "cafebabecafebabe"
            }),
        ];

        let schema = infer(&samples);
        assert_eq!(schema.samples, 2);

        let field = |name: &str| {
            schema
                .field(&Address::parse(name).unwrap())
                .unwrap_or_else(|| panic!("field {name}"))
        };

        assert_eq!(field("fixed").volatility, Volatility::Constant);
        assert_eq!(field("id").volatility, Volatility::Uuid);
        assert_eq!(field("clock").volatility, Volatility::Clock);
        assert_eq!(field("count").volatility, Volatility::Counter);
        assert_eq!(field("digest").volatility, Volatility::Digest);
    }

    #[test]
    fn records_shapes_and_ranges() {
        let samples = vec![json!({ "v": 1 }), json!({ "v": 9.5 })];
        let schema = infer(&samples);
        let field = schema.field(&Address::parse("v").unwrap()).unwrap();

        assert!(field.shapes.contains(&LeafShape::Integer));
        assert!(field.shapes.contains(&LeafShape::Float));
        assert_eq!(field.min, Some(1.0));
        assert_eq!(field.max, Some(9.5));
    }

    #[test]
    fn reports_missing_fields() {
        let samples = vec![json!({ "a": 1, "b": 2 }), json!({ "a": 1 })];
        let schema = infer(&samples);

        assert_eq!(schema.field(&Address::parse("a").unwrap()).unwrap().present_in, 2);
        assert_eq!(schema.field(&Address::parse("b").unwrap()).unwrap().present_in, 1);
        assert_eq!(schema.always_present().len(), 1);
    }
}
