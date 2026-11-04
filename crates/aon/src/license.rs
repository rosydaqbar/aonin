//! Aonin Pro license keys.
//!
//! A key is `AONIN-PRO-` + base64url(payload JSON) + `.` + base64url(Ed25519
//! signature of the payload). Only the holder of the signing key can make
//! one; anyone can check one with the public key below, offline.
//!
//! A license is for life, with a year of updates: Pro works in every build
//! released up to `updates_until`.

use anyhow::{Result, anyhow};
use base64::Engine as _;
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use ed25519_dalek::{Signature, VerifyingKey};
use serde::{Deserialize, Serialize};

const PREFIX: &str = "AONIN-PRO-";

/// Checks every key. Its private half never leaves the key holder.
const PUBLIC_KEY: [u8; 32] = [
    0x02, 0x19, 0x70, 0xdb, 0x48, 0xcd, 0x9c, 0xf0, 0x7c, 0xd2, 0x40, 0x36, 0x2e, 0x7e, 0x4f, 0xaa, 0x23, 0x33, 0x05, 0xde, 0x8d,
    0xd5, 0xc6, 0x40, 0xdb, 0xee, 0x35, 0x77, 0xc8, 0x01, 0x99, 0xa6,
];

#[derive(Serialize, Clone, Debug, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct License {
    pub id: String,
    pub email: String,
    /// Dates as YYYY-MM-DD, so they compare as strings.
    pub issued: String,
    pub updates_until: String,
}

#[derive(Deserialize)]
struct Payload {
    v: u32,
    id: String,
    email: String,
    issued: String,
    updates_until: String,
}

/// The license in `key`, if the key is genuine.
pub fn verify(key: &str) -> Result<License> {
    verify_with(key, &PUBLIC_KEY)
}

fn verify_with(key: &str, public: &[u8; 32]) -> Result<License> {
    let invalid = || anyhow!("That license key isn't valid. Check it and try again.");
    let body: String = key.split_whitespace().collect();
    let body = body.strip_prefix(PREFIX).ok_or_else(invalid)?;
    let (payload, sig) = body.split_once('.').ok_or_else(invalid)?;
    let payload = URL_SAFE_NO_PAD.decode(payload).map_err(|_| invalid())?;
    let sig: [u8; 64] = URL_SAFE_NO_PAD.decode(sig).map_err(|_| invalid())?.try_into().map_err(|_| invalid())?;
    let public = VerifyingKey::from_bytes(public).map_err(|_| invalid())?;
    public.verify_strict(&payload, &Signature::from_bytes(&sig)).map_err(|_| invalid())?;
    let p: Payload = serde_json::from_slice(&payload).map_err(|_| invalid())?;
    if p.v != 1 {
        return Err(anyhow!("This license key needs a newer version of Aonin."));
    }
    Ok(License { id: p.id, email: p.email, issued: p.issued, updates_until: p.updates_until })
}

/// Whether `license` covers a build released on `release_date` (YYYY-MM-DD).
pub fn covers(license: &License, release_date: &str) -> bool {
    release_date <= license.updates_until.as_str()
}

#[cfg(test)]
mod tests {
    use super::*;
    use ed25519_dalek::{Signer, SigningKey};

    fn key_for(signer: &SigningKey, payload: &str) -> String {
        let sig = signer.sign(payload.as_bytes());
        format!("{PREFIX}{}.{}", URL_SAFE_NO_PAD.encode(payload), URL_SAFE_NO_PAD.encode(sig.to_bytes()))
    }

    const PAYLOAD: &str = r#"{"v":1,"id":"a1b2c3","email":"me@example.com","issued":"2026-11-04","updates_until":"2027-11-04"}"#;

    #[test]
    fn genuine_key_is_accepted() {
        let signer = SigningKey::from_bytes(&[7; 32]);
        let key = key_for(&signer, PAYLOAD);
        let l = verify_with(&key, signer.verifying_key().as_bytes()).unwrap();
        assert_eq!(l.email, "me@example.com");
        assert_eq!(l.updates_until, "2027-11-04");
        // Pasted with line breaks or spaces still works.
        let wrapped = format!("  {}\n{}  ", &key[..30], &key[30..]);
        assert!(verify_with(&wrapped, signer.verifying_key().as_bytes()).is_ok());
    }

    #[test]
    fn changed_or_foreign_keys_are_refused() {
        let signer = SigningKey::from_bytes(&[7; 32]);
        let key = key_for(&signer, PAYLOAD);
        // Someone else's signing key.
        let other = SigningKey::from_bytes(&[8; 32]);
        assert!(verify_with(&key_for(&other, PAYLOAD), signer.verifying_key().as_bytes()).is_err());
        // A longer update period pasted in, old signature kept.
        let longer = PAYLOAD.replace("2027", "2099");
        let (_, sig) = key.split_once('.').unwrap();
        let forged = format!("{PREFIX}{}.{sig}", URL_SAFE_NO_PAD.encode(longer));
        assert!(verify_with(&forged, signer.verifying_key().as_bytes()).is_err());
        assert!(verify("AONIN-PRO-nonsense").is_err());
        assert!(verify("").is_err());
    }

    #[test]
    fn update_period() {
        let l = License { id: "x".into(), email: "e".into(), issued: "2026-11-04".into(), updates_until: "2027-11-04".into() };
        assert!(covers(&l, "2026-12-01"));
        assert!(covers(&l, "2027-11-04"));
        assert!(!covers(&l, "2027-11-05"));
    }
}
