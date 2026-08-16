use base64::Engine;
use base64::engine::general_purpose::STANDARD;
use rand::RngExt;
use serde_json::{Value, json};
use sha1::Sha1;
use sha2::{Digest, Sha256, Sha384, Sha512};

use wre_core::error::{Error, Result};
use wre_live::realm::Realm;

pub fn install(realm: &mut Realm) -> Result<()> {
    realm.register_host(
        "__wreEntropy",
        Box::new(|args| {
            let wanted = args.first().and_then(Value::as_u64).unwrap_or_default() as usize;
            let mut bytes = vec![0u8; wanted.min(65_536)];
            rand::rng().fill(&mut bytes[..]);
            Ok(json!(STANDARD.encode(bytes)))
        }),
    )?;

    realm.register_host(
        "__wreDigest",
        Box::new(|args| {
            let algorithm = args.first().and_then(Value::as_str).unwrap_or_default();
            let encoded = args.get(1).and_then(Value::as_str).unwrap_or_default();

            let bytes = STANDARD
                .decode(encoded)
                .map_err(|error| Error::msg(format!("the digest input was not base64: {error}")))?;

            let out = digest_of(algorithm, &bytes)
                .ok_or_else(|| Error::msg(format!("unsupported algorithm {algorithm}")))?;

            Ok(json!(STANDARD.encode(out)))
        }),
    )?;

    Ok(())
}

pub fn digest_of(algorithm: &str, bytes: &[u8]) -> Option<Vec<u8>> {
    let name: String = algorithm
        .chars()
        .filter(|character| character.is_ascii_alphanumeric())
        .collect::<String>()
        .to_ascii_lowercase();

    match name.as_str() {
        "sha1" => Some(Sha1::digest(bytes).to_vec()),
        "sha256" => Some(Sha256::digest(bytes).to_vec()),
        "sha384" => Some(Sha384::digest(bytes).to_vec()),
        "sha512" => Some(Sha512::digest(bytes).to_vec()),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_algorithm_name_is_read_the_way_webcrypto_spells_it() {
        let empty = digest_of("SHA-256", b"").expect("sha-256");
        assert_eq!(
            hex::encode(empty),
            "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855"
        );

        assert_eq!(digest_of("sha256", b"abc"), digest_of("SHA-256", b"abc"));
        assert_eq!(digest_of("SHA-1", b"").map(|out| out.len()), Some(20));
        assert_eq!(digest_of("SHA-384", b"").map(|out| out.len()), Some(48));
        assert_eq!(digest_of("SHA-512", b"").map(|out| out.len()), Some(64));
        assert!(digest_of("MD5", b"").is_none());
    }
}
