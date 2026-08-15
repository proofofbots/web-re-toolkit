use wre_core::error::{Error, Result};

pub fn rotate_digits(text: &str, key: &str, forward: bool) -> Result<String> {
    let steps: Vec<u32> = key
        .chars()
        .filter_map(|symbol| symbol.to_digit(10))
        .collect();

    if steps.is_empty() {
        return Err(Error::msg("the key carries no digits"));
    }

    let mut out = String::with_capacity(text.len());
    let mut position = 0usize;

    for symbol in text.chars() {
        match symbol.to_digit(10) {
            Some(digit) => {
                let step = steps[position % steps.len()];
                let moved = if forward {
                    (digit + step) % 10
                } else {
                    (digit + 10 - step % 10) % 10
                };
                out.push(char::from_digit(moved, 10).unwrap_or(symbol));
                position += 1;
            }
            None => out.push(symbol),
        }
    }

    Ok(out)
}

pub fn rotate_alphabet(text: &str, alphabet: &[char], steps: &[i64]) -> String {
    if alphabet.is_empty() || steps.is_empty() {
        return text.to_string();
    }

    let width = alphabet.len() as i64;
    let mut out = String::with_capacity(text.len());
    let mut position = 0usize;

    for symbol in text.chars() {
        match alphabet.iter().position(|entry| *entry == symbol) {
            Some(index) => {
                let step = steps[position % steps.len()];
                let moved = (index as i64 + step).rem_euclid(width) as usize;
                out.push(alphabet[moved]);
                position += 1;
            }
            None => out.push(symbol),
        }
    }

    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn digit_rotation_inverts() {
        let sealed = rotate_digits("1700000000123", "4821", true).unwrap();
        assert_ne!(sealed, "1700000000123");
        assert_eq!(rotate_digits(&sealed, "4821", false).unwrap(), "1700000000123");
    }

    #[test]
    fn non_digits_pass_through_without_consuming_a_step() {
        let sealed = rotate_digits("a1b2", "11", true).unwrap();
        assert_eq!(sealed, "a2b3");
    }

    #[test]
    fn a_key_with_no_digits_is_rejected() {
        assert!(rotate_digits("123", "abc", true).is_err());
    }

    #[test]
    fn alphabet_rotation_inverts() {
        let alphabet: Vec<char> = "abcdefghijklmnopqrstuvwxyz".chars().collect();
        let sealed = rotate_alphabet("session", &alphabet, &[3, 1, 4]);
        assert_ne!(sealed, "session");
        assert_eq!(rotate_alphabet(&sealed, &alphabet, &[-3, -1, -4]), "session");
    }

    #[test]
    fn an_empty_alphabet_leaves_the_text_alone() {
        assert_eq!(rotate_alphabet("text", &[], &[1]), "text");
        assert_eq!(rotate_alphabet("text", &['a'], &[]), "text");
    }
}
