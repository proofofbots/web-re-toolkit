use base64::Engine;
use base64::engine::general_purpose::STANDARD;
use serde_json::{Map, Value, json};

pub const KEY: &str = "omgtopkek";

pub fn decode(body: &[u8]) -> Option<Value> {
    let data = payload(body)?;
    let bytes = STANDARD.decode(data.trim()).ok()?;

    if let Some(found) = with_key(&bytes, KEY.as_bytes()) {
        return Some(found);
    }

    let recovered = recover_key(&bytes)?;
    with_key(&bytes, &recovered)
}

pub fn flagged(report: &Value) -> Vec<Value> {
    let Some(fields) = report.as_object() else {
        return Vec::new();
    };

    fields
        .iter()
        .filter_map(|(name, value)| {
            let entry = value.as_object()?;

            let why = if let Some(problems) = entry.get("problems") {
                format!("problems={problems}")
            } else if let Some(verdict) = entry.get("v") {
                format!("v={verdict}")
            } else if let Some(error) = entry.get("e") {
                format!("e={error}")
            } else {
                return None;
            };

            Some(json!({
                "check": name,
                "why": why,
                "raw": entry.get("r").cloned().unwrap_or(Value::Null),
            }))
        })
        .collect()
}

pub fn about(report: &Value) -> Value {
    let read = |name: &str| report.get(name).cloned().unwrap_or(Value::Null);

    json!({
        "build": read("bid"),
        "version": read("_v"),
        "origin": read("_rlj"),
        "checks": report.as_object().map(count_checks).unwrap_or_default(),
    })
}

fn count_checks(fields: &Map<String, Value>) -> usize {
    fields
        .values()
        .filter(|value| value.is_object() && !value.is_array())
        .count()
}

fn payload(body: &[u8]) -> Option<String> {
    let text = String::from_utf8_lossy(body).to_string();
    let mut candidates = vec![text.clone()];

    if let Ok(decoded) = STANDARD.decode(text.trim()) {
        candidates.push(String::from_utf8_lossy(&decoded).to_string());
    }

    for candidate in candidates {
        if let Ok(Value::Object(fields)) = serde_json::from_str::<Value>(&candidate)
            && let Some(Value::String(data)) = fields.get("data")
        {
            return Some(data.clone());
        }
    }

    None
}

fn with_key(bytes: &[u8], key: &[u8]) -> Option<Value> {
    if key.is_empty() {
        return None;
    }

    let plain: Vec<u8> = bytes
        .iter()
        .enumerate()
        .map(|(index, byte)| byte ^ key[index % key.len()])
        .collect();

    serde_json::from_slice(&plain).ok()
}

fn recover_key(bytes: &[u8]) -> Option<Vec<u8>> {
    let period = period_of(bytes)?;
    let mut key = Vec::with_capacity(period);

    for column in 0..period {
        let mut best = (0u8, f64::MIN);

        for candidate in 0..=255u8 {
            let total: f64 = bytes
                .iter()
                .skip(column)
                .step_by(period)
                .map(|byte| weight(byte ^ candidate))
                .sum();

            if total > best.1 {
                best = (candidate, total);
            }
        }

        key.push(best.0);
    }

    Some(key)
}

fn period_of(bytes: &[u8]) -> Option<usize> {
    let coincidence = |period: usize| -> f64 {
        let mut same = 0usize;
        let mut total = 0usize;

        for index in 0..bytes.len().saturating_sub(period) {
            total += 1;
            if bytes[index] == bytes[index + period] {
                same += 1;
            }
        }

        if total == 0 {
            0.0
        } else {
            same as f64 / total as f64
        }
    };

    let scores: Vec<f64> = (1..=64).map(coincidence).collect();
    let mean: f64 = scores.iter().sum::<f64>() / scores.len() as f64;

    scores
        .iter()
        .position(|score| *score > mean * 2.5)
        .map(|index| index + 1)
}

fn weight(byte: u8) -> f64 {
    let printable = (32..127).contains(&byte) || matches!(byte, 9 | 10 | 13);

    if !printable {
        return -100.0;
    }

    match byte {
        32 => 6.0,
        b'a'..=b'z' => 5.0,
        b'{' | b'}' | b'[' | b']' | b'"' | b':' | b',' => 4.0,
        _ => 1.0,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn posted(report: &str, key: &[u8]) -> Vec<u8> {
        let sealed: Vec<u8> = report
            .bytes()
            .enumerate()
            .map(|(index, byte)| byte ^ key[index % key.len()])
            .collect();

        format!("{{\"data\":\"{}\"}}", STANDARD.encode(sealed)).into_bytes()
    }

    const REPORT: &str = r#"{"bid":"build-7","_v":"j-1.2.661","_rlj":"https://www.example.com",
        "elc":{"v":true,"r":"9"},"dpv":{"b":1},"wrc":{"e":"threw"}}"#;

    #[test]
    fn the_known_key_reads_the_report() {
        let found = decode(&posted(REPORT, KEY.as_bytes())).expect("no report");
        assert_eq!(found["bid"], "build-7");
        assert_eq!(about(&found)["version"], "j-1.2.661");
    }

    #[test]
    fn a_build_that_rotated_the_key_still_decodes() {
        let mut long = String::from("{\"bid\":\"build-7\"");
        for index in 0..80 {
            long.push_str(&format!(
                ",\"check{index}\":{{\"b\":1,\"r\":\"a value here\"}}"
            ));
        }
        long.push('}');

        let key = b"another-key";
        let found = decode(&posted(&long, key)).expect("no report");

        assert_eq!(found["bid"], "build-7");
        assert_eq!(
            recover_key(
                &STANDARD
                    .decode(payload(&posted(&long, key)).unwrap())
                    .unwrap()
            ),
            Some(key.to_vec())
        );
    }

    #[test]
    fn only_the_checks_that_said_something_are_flagged() {
        let found = decode(&posted(REPORT, KEY.as_bytes())).expect("no report");
        let names: Vec<String> = flagged(&found)
            .iter()
            .map(|entry| entry["check"].as_str().unwrap_or_default().to_string())
            .collect();

        assert!(names.contains(&"elc".to_string()));
        assert!(names.contains(&"wrc".to_string()));
        assert!(!names.contains(&"dpv".to_string()));
    }

    #[test]
    fn a_body_that_is_not_a_report_decodes_to_nothing() {
        assert!(decode(b"not a report").is_none());
        assert!(decode(br#"{"data":"!!!!"}"#).is_none());
    }
}
