use base64::Engine;
use base64::engine::general_purpose::STANDARD;
use serde_json::{Map, Value};

const JSON_FIELDS: [&str; 7] = ["bt", "timing", "sr", "dp", "nav", "crc", "z"];

pub fn decode_query(url: &str) -> Option<Map<String, Value>> {
    let query = url.split_once('?')?.1;
    let packed = form_pairs(query)
        .into_iter()
        .find(|(name, _)| name == "a")
        .map(|(_, value)| value)?;

    let decoded = String::from_utf8(STANDARD.decode(packed).ok()?).ok()?;

    let mut out = Map::new();
    for (name, value) in form_pairs(&decoded) {
        out.insert(name, Value::String(value));
    }

    Some(out)
}

pub fn parse_post(body: &str) -> Map<String, Value> {
    let mut out = Map::new();

    for (name, value) in form_pairs(body) {
        if JSON_FIELDS.contains(&name.as_str())
            && let Ok(parsed) = serde_json::from_str::<Value>(&value)
        {
            out.insert(name, parsed);
            continue;
        }

        out.insert(name, Value::String(value));
    }

    out
}

pub fn form_pairs(text: &str) -> Vec<(String, String)> {
    text.split('&')
        .filter(|part| !part.is_empty())
        .map(|part| match part.split_once('=') {
            Some((name, value)) => (decode(name), decode(value)),
            None => (decode(part), String::new()),
        })
        .collect()
}

fn decode(text: &str) -> String {
    let bytes = text.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut index = 0;

    while index < bytes.len() {
        match bytes[index] {
            b'+' => {
                out.push(b' ');
                index += 1;
            }
            b'%' if index + 2 < bytes.len() => {
                let high = (bytes[index + 1] as char).to_digit(16);
                let low = (bytes[index + 2] as char).to_digit(16);

                match (high, low) {
                    (Some(high), Some(low)) => {
                        out.push((high * 16 + low) as u8);
                        index += 3;
                    }
                    _ => {
                        out.push(bytes[index]);
                        index += 1;
                    }
                }
            }
            byte => {
                out.push(byte);
                index += 1;
            }
        }
    }

    String::from_utf8_lossy(&out).into_owned()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_tag_query_decodes_to_the_token_and_the_js_flag() {
        let fields = decode_query("https://www.maersk.com/akam/13/pixel_4ebc0144?a=dD0xJmpzPW9mZg==")
            .expect("query");

        assert_eq!(fields.get("t").and_then(Value::as_str), Some("1"));
        assert_eq!(fields.get("js").and_then(Value::as_str), Some("off"));
    }

    #[test]
    fn a_post_body_splits_with_its_json_fields_parsed() {
        let body = "ap=true&sr=%7B%22inner%22%3A%5B1512%2C895%5D%7D&br=Chrome&z=%7B%22a%22%3A5%7D";
        let fields = parse_post(body);

        assert_eq!(fields.get("ap").and_then(Value::as_str), Some("true"));
        assert_eq!(fields.get("br").and_then(Value::as_str), Some("Chrome"));
        assert_eq!(fields["sr"]["inner"][0], 1512);
        assert_eq!(fields["z"]["a"], 5);
    }
}
