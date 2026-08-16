use serde::{Deserialize, Serialize};

pub const AKAMAI_COOKIES: [&str; 9] =
    ["_abck", "bm_sz", "ak_bmsc", "bm_sv", "bm_mi", "bm_so", "bm_s", "bm_lso", "sec_cpt"];

pub fn is_akamai(name: &str) -> bool {
    AKAMAI_COOKIES.contains(&name) || name.starts_with("ak_") || name.starts_with("bm_")
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum State {
    None,
    Validated,
    Unvalidated,
    Unknown,
}

impl State {
    pub fn as_str(self) -> &'static str {
        match self {
            State::None => "none",
            State::Validated => "validated",
            State::Unvalidated => "unvalidated",
            State::Unknown => "unknown",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Abck {
    pub token: String,
    pub status: String,
    pub fields: Vec<String>,
    pub validated: bool,
    pub rejected: bool,
    pub challenges: Vec<String>,
}

pub fn parse_abck(value: &str) -> Abck {
    let decoded = decode(value);
    let fields: Vec<String> = decoded.split('~').map(str::to_string).collect();
    let status = fields.get(1).cloned().unwrap_or_default();

    let challenges = fields
        .get(4)
        .map(|field| {
            field
                .split("||")
                .filter(|entry| !entry.is_empty() && *entry != "-1")
                .map(str::to_string)
                .collect()
        })
        .unwrap_or_default();

    Abck {
        token: fields.first().cloned().unwrap_or_default(),
        validated: status == "0" || status == "0=",
        rejected: status == "-1",
        status,
        challenges,
        fields,
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct BmSz {
    pub token: String,
    pub blob: String,
    pub fields: Vec<String>,
}

pub fn parse_bm_sz(value: &str) -> BmSz {
    let fields: Vec<String> = value.split('~').map(str::to_string).collect();

    BmSz {
        token: fields.first().cloned().unwrap_or_default(),
        blob: fields.get(1).cloned().unwrap_or_default(),
        fields,
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SecCpt {
    pub fields: Vec<String>,
    pub token: String,
}

pub fn parse_sec_cpt(value: &str) -> SecCpt {
    let fields: Vec<String> = value.split('~').map(str::to_string).collect();
    SecCpt { token: fields.first().cloned().unwrap_or_default(), fields }
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct Summary {
    pub present: Vec<String>,
    pub abck: Option<Abck>,
    pub bm_sz: Option<BmSz>,
    pub sec_cpt: Option<SecCpt>,
    pub state: String,
    pub challenged: bool,
}

pub fn summarise(pairs: &[(String, String)]) -> Summary {
    let present: Vec<String> = pairs
        .iter()
        .map(|(name, _)| name.clone())
        .filter(|name| is_akamai(name))
        .collect();

    let find = |wanted: &str| {
        pairs
            .iter()
            .find(|(name, _)| name == wanted)
            .map(|(_, value)| value.clone())
    };

    let abck = find("_abck").map(|value| parse_abck(&value));
    let bm_sz = find("bm_sz").map(|value| parse_bm_sz(&value));
    let sec_cpt = find("sec_cpt").map(|value| parse_sec_cpt(&value));

    let state = match &abck {
        None => State::None,
        Some(found) if found.validated => State::Validated,
        Some(found) if found.rejected => State::Unvalidated,
        Some(_) => State::Unknown,
    };

    let challenged = sec_cpt.is_some()
        || abck.as_ref().map(|found| !found.challenges.is_empty()).unwrap_or(false);

    Summary {
        present,
        abck,
        bm_sz,
        sec_cpt,
        state: state.as_str().to_string(),
        challenged,
    }
}

pub fn parse_header(header: &str) -> Vec<(String, String)> {
    header
        .split(';')
        .filter_map(|part| part.trim().split_once('='))
        .map(|(name, value)| (name.trim().to_string(), value.to_string()))
        .collect()
}

fn decode(value: &str) -> String {
    let bytes = value.as_bytes();
    let mut out = String::with_capacity(value.len());
    let mut index = 0;

    while index < bytes.len() {
        if bytes[index] == b'%' && index + 2 < bytes.len() {
            let high = (bytes[index + 1] as char).to_digit(16);
            let low = (bytes[index + 2] as char).to_digit(16);

            if let (Some(high), Some(low)) = (high, low) {
                out.push(((high * 16 + low) as u8) as char);
                index += 3;
                continue;
            }
        }

        out.push(bytes[index] as char);
        index += 1;
    }

    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_unvalidated_abck_reads_as_unvalidated() {
        let summary = summarise(&parse_header(
            "bm_sz=A1B2~YAAQ~3~4; _abck=6C0E~-1~gYr8~-1~-1~-1; other=x",
        ));

        assert_eq!(summary.state, "unvalidated");
        assert_eq!(summary.present, vec!["bm_sz", "_abck"]);
        assert!(!summary.challenged);

        let abck = summary.abck.expect("abck");
        assert_eq!(abck.token, "6C0E");
        assert!(abck.rejected);
        assert!(abck.challenges.is_empty());
    }

    #[test]
    fn a_validated_abck_reads_as_validated() {
        let summary = summarise(&parse_header("_abck=6C0E~0~gYr8~-1~-1~-1"));
        assert_eq!(summary.state, "validated");
        assert!(summary.abck.unwrap().validated);
    }

    #[test]
    fn a_work_item_in_field_four_is_a_challenge() {
        let summary = summarise(&parse_header("_abck=TOKEN~-1~salt~-1~3-abc-500-1000-30-2||4-def-700-1000-30"));

        assert!(summary.challenged);
        assert_eq!(summary.abck.unwrap().challenges.len(), 2);
    }

    #[test]
    fn a_percent_encoded_cookie_is_read_after_decoding() {
        let abck = parse_abck("TOKEN%7E-1%7Esalt%7E-1%7E-1");
        assert_eq!(abck.token, "TOKEN");
        assert!(abck.rejected);
    }

    #[test]
    fn a_sec_cpt_cookie_marks_the_session_challenged() {
        let summary = summarise(&parse_header("sec_cpt=abc~1~2"));
        assert!(summary.challenged);
        assert_eq!(summary.sec_cpt.unwrap().token, "abc");
    }
}
