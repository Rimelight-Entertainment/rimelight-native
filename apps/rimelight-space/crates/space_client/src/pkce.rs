//! PKCE (RFC 7636) verifier, S256 challenge and the sign-in `state` value.

use base64::Engine as _;
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use sha2::{Digest, Sha256};

use crate::error::ClientError;

/// RFC 7636 unreserved characters: ALPHA / DIGIT / "-" / "." / "_" / "~".
const ALPHABET: &[u8] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789-._~";
/// Verifier length, inside the 43..=128 range the RFC allows.
const VERIFIER_LEN: usize = 64;
/// 66 symbols; accepting only bytes below 198 (= 66 * 3) avoids modulo bias.
const ACCEPT_BELOW: u8 = 198;

/// A verifier kept by the app and the challenge sent to the browser login.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PkcePair {
    pub verifier: String,
    pub challenge: String,
}

/// Creates a fresh verifier and its S256 challenge.
pub fn new_pair() -> Result<PkcePair, ClientError> {
    let verifier = new_verifier()?;
    let challenge = challenge_for(&verifier);
    Ok(PkcePair {
        verifier,
        challenge,
    })
}

/// Random 64-character verifier from the unreserved alphabet.
pub fn new_verifier() -> Result<String, ClientError> {
    let mut out = String::with_capacity(VERIFIER_LEN);
    let mut buf = [0u8; 96];
    while out.len() < VERIFIER_LEN {
        random_fill(&mut buf)?;
        for &b in &buf {
            if out.len() == VERIFIER_LEN {
                break;
            }
            if b < ACCEPT_BELOW {
                out.push(char::from(ALPHABET[usize::from(b) % ALPHABET.len()]));
            }
        }
    }
    Ok(out)
}

/// S256 challenge: base64url (no padding) of SHA-256 of the verifier.
pub fn challenge_for(verifier: &str) -> String {
    URL_SAFE_NO_PAD.encode(Sha256::digest(verifier.as_bytes()))
}

/// Random 32-byte `state`, base64url without padding (43 characters).
pub fn new_state() -> Result<String, ClientError> {
    let mut buf = [0u8; 32];
    random_fill(&mut buf)?;
    Ok(URL_SAFE_NO_PAD.encode(buf))
}

fn random_fill(buf: &mut [u8]) -> Result<(), ClientError> {
    getrandom::fill(buf).map_err(|e| ClientError::Random(e.to_string()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn s256_challenge_matches_rfc7636_appendix_b() {
        let verifier = "dBjftJeZ4CVP-mB92K27uhbUJU1p1r_wW1gFWFOEjXk";
        assert_eq!(
            challenge_for(verifier),
            "E9Melhoa2OwvFrEMTJguCHaoeK1t8URWbuGJSstw-cM"
        );
    }

    #[test]
    fn verifier_has_valid_length_and_charset() {
        for _ in 0..20 {
            let v = new_verifier().expect("random");
            assert_eq!(v.len(), VERIFIER_LEN);
            assert!((43..=128).contains(&v.len()));
            assert!(v.bytes().all(|b| ALPHABET.contains(&b)), "bad char in {v}");
        }
    }

    #[test]
    fn pair_challenge_is_derived_from_verifier() {
        let pair = new_pair().expect("random");
        assert_eq!(pair.challenge, challenge_for(&pair.verifier));
    }

    #[test]
    fn state_is_43_chars_and_varies() {
        let a = new_state().expect("random");
        let b = new_state().expect("random");
        assert_eq!(a.len(), 43);
        assert_ne!(a, b);
    }
}
