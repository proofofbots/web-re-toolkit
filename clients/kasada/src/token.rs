use base64::Engine;
use base64::engine::general_purpose::STANDARD_NO_PAD;
use serde::{Deserialize, Serialize};

pub const COOKIE: &str = "KP_UIDz";
pub const SESSION_COOKIE: &str = "KP_UIDz-ssn";

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Verdict {
    Solved,
    Unsolved,
    None,
    Unknown,
}

impl Verdict {
    pub fn as_str(self) -> &'static str {
        match self {
            Verdict::Solved => "solved",
            Verdict::Unsolved => "unsolved",
            Verdict::None => "none",
            Verdict::Unknown => "unknown",
        }
    }
}

pub fn verdict(issued: Option<&str>, accepted: Option<bool>) -> Verdict {
    let Some(token) = issued.filter(|value| !value.is_empty()) else {
        return Verdict::None;
    };

    if accepted == Some(true) {
        return Verdict::Solved;
    }

    match decoded_len(token) {
        Some(129) => Verdict::Solved,
        Some(131) => Verdict::Unsolved,
        _ => Verdict::Unknown,
    }
}

pub fn decoded_len(token: &str) -> Option<usize> {
    let standard: String = token
        .chars()
        .map(|character| match character {
            '-' => '+',
            '_' => '/',
            other => other,
        })
        .filter(|character| *character != '=')
        .collect();

    STANDARD_NO_PAD
        .decode(standard)
        .ok()
        .map(|bytes| bytes.len())
}

pub fn jar(token: &str) -> String {
    format!("{COOKIE}={token}; {SESSION_COOKIE}={token}")
}

pub fn headers(token: &str, version: &str, h: Option<&str>) -> Vec<(String, String)> {
    let mut out = vec![
        ("x-kpsdk-ct".to_string(), token.to_string()),
        ("x-kpsdk-v".to_string(), version.to_string()),
    ];

    if let Some(value) = h.filter(|value| !value.is_empty()) {
        out.push(("x-kpsdk-h".to_string(), value.to_string()));
    }

    out
}

pub fn kasada_headers(headers: &[(String, String)]) -> Vec<(String, String)> {
    headers
        .iter()
        .filter(|(name, _)| name.to_ascii_lowercase().starts_with("x-kpsdk-"))
        .map(|(name, value)| (name.to_ascii_lowercase(), value.clone()))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use base64::engine::general_purpose::URL_SAFE_NO_PAD;

    fn token_of(len: usize) -> String {
        URL_SAFE_NO_PAD.encode(vec![7u8; len])
    }

    #[test]
    fn a_hundred_and_twenty_nine_byte_token_is_a_solve() {
        assert_eq!(verdict(Some(&token_of(129)), None), Verdict::Solved);
        assert_eq!(verdict(Some(&token_of(131)), None), Verdict::Unsolved);
        assert_eq!(verdict(Some(&token_of(64)), None), Verdict::Unknown);
    }

    #[test]
    fn the_accept_header_wins_over_the_length() {
        assert_eq!(verdict(Some(&token_of(131)), Some(true)), Verdict::Solved);
        assert_eq!(verdict(None, Some(true)), Verdict::None);
        assert_eq!(verdict(Some(""), Some(true)), Verdict::None);
    }

    #[test]
    fn the_jar_binds_both_names_to_the_same_token() {
        assert_eq!(jar("abc"), "KP_UIDz=abc; KP_UIDz-ssn=abc");
    }

    #[test]
    fn headers_carry_the_build_and_drop_an_empty_hash() {
        let with = headers("token", "j-1.2.661", Some("01"));
        assert_eq!(with.len(), 3);
        assert_eq!(with[2], ("x-kpsdk-h".to_string(), "01".to_string()));
        assert_eq!(headers("token", "j-1.2.661", Some("")).len(), 2);
    }

    #[test]
    fn only_the_vendor_headers_are_kept() {
        let all = vec![
            ("X-KPSDK-CT".to_string(), "token".to_string()),
            ("server".to_string(), "kasada".to_string()),
        ];

        assert_eq!(
            kasada_headers(&all),
            vec![("x-kpsdk-ct".to_string(), "token".to_string())]
        );
    }
}
