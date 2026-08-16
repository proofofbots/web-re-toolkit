use base64::Engine;
use base64::engine::general_purpose::STANDARD;

const OPENER: &str = "{\"sensor_data\":\"";
const FIELD: &str = "sensor_data=";
const SEPARATOR: &str = "&&&";

pub fn extract(body: &str) -> Option<String> {
    if let Ok(parsed) = serde_json::from_str::<serde_json::Value>(body)
        && let Some(found) = parsed.get("sensor_data").and_then(|value| value.as_str())
    {
        return Some(found.to_string());
    }

    let start = body.find(OPENER)?;
    let from = start + OPENER.len();
    let end = body.rfind("\"}");

    match end {
        Some(end) if end > from => Some(body[from..end].to_string()),
        _ => Some(body[from..].to_string()),
    }
}

pub fn wrap(payload: &str) -> String {
    serde_json::json!({ "sensor_data": payload }).to_string()
}

pub fn payload_of(header: &str) -> Option<String> {
    let field = header.split(SEPARATOR).find(|part| part.starts_with(FIELD))?;
    let encoded = field.trim_start_matches(FIELD);
    let decoded = STANDARD.decode(encoded).ok()?;

    String::from_utf8(decoded).ok()
}

pub fn with_payload(header: &str, payload: &str) -> String {
    let mut fields: Vec<String> = header
        .split(SEPARATOR)
        .filter(|part| !part.starts_with(FIELD))
        .map(str::to_string)
        .collect();

    fields.push(format!("{FIELD}{}", STANDARD.encode(payload.as_bytes())));
    fields.join(SEPARATOR)
}

pub fn looks_like_payload(body: &str) -> bool {
    body.contains("sensor_data")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_payload_comes_out_of_a_post_body() {
        let body = r#"{"sensor_data":"7a74G7m23Vrp0o5c9XJ~1~abc"}"#;
        assert_eq!(extract(body).as_deref(), Some("7a74G7m23Vrp0o5c9XJ~1~abc"));
        assert!(looks_like_payload(body));
    }

    #[test]
    fn a_payload_with_a_quote_in_it_still_comes_out() {
        let body = "{\"sensor_data\":\"abc\\\"def\"}";
        assert_eq!(extract(body).as_deref(), Some("abc\"def"));
    }

    #[test]
    fn wrapping_round_trips() {
        let payload = "7a74G7m23Vrp";
        assert_eq!(extract(&wrap(payload)).as_deref(), Some(payload));
    }

    #[test]
    fn the_telemetry_header_carries_the_payload_in_base64() {
        let header = "a=1&&&b=2&&&sensor_data=N2E3NEc3bTIzVnJw";

        assert_eq!(payload_of(header).as_deref(), Some("7a74G7m23Vrp"));

        let replaced = with_payload(header, "garbage");
        assert_eq!(replaced, "a=1&&&b=2&&&sensor_data=Z2FyYmFnZQ==");
        assert_eq!(payload_of(&replaced).as_deref(), Some("garbage"));
    }
}
