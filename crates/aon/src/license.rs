//! Aonin Pro license keys, checked offline.
//!
//! A key is `AONIN-` + base64url of 71 bytes:
//!   version (1, = 2) · id (4) · last update day (u16, days since 2026-01-01)
//!   · Ed25519 signature (64) of "aonin-pro" + the 7 bytes before it.
//! Only the holder of the signing key can make one; anyone can check one
//! with the public key below.
//!
//! A license is for life, with a year of updates: Pro works in every build
//! released up to `updates_until`.

use anyhow::{Result, anyhow};
use base64::Engine as _;
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use ed25519_dalek::{Signature, VerifyingKey};
use serde::Serialize;

const PREFIX: &str = "AONIN-";
const VERSION: u8 = 2;
/// Signed together with the payload, so the signature means nothing elsewhere.
const CONTEXT: &[u8] = b"aonin-pro";
/// Day 0 of `updates_until`.
const EPOCH_DAYS: i64 = days_from_civil(2026, 1, 1);

/// Checks every key. Its private half never leaves the key holder.
const PUBLIC_KEY: [u8; 32] = [
    0x02, 0x19, 0x70, 0xdb, 0x48, 0xcd, 0x9c, 0xf0, 0x7c, 0xd2, 0x40, 0x36, 0x2e, 0x7e, 0x4f, 0xaa, 0x23, 0x33, 0x05, 0xde, 0x8d,
    0xd5, 0xc6, 0x40, 0xdb, 0xee, 0x35, 0x77, 0xc8, 0x01, 0x99, 0xa6,
];

#[derive(Serialize, Clone, Debug, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct License {
    /// 8 hex digits, for support and for revoking a leaked key.
    pub id: String,
    /// The last release date this license gets (YYYY-MM-DD).
    pub updates_until: String,
}

/// The license in `key`, if the key is genuine.
pub fn verify(key: &str) -> Result<License> {
    verify_with(key, &PUBLIC_KEY)
}

fn verify_with(key: &str, public: &[u8; 32]) -> Result<License> {
    let invalid = || anyhow!("That license key isn't valid. Check it and try again.");
    let body: String = key.split_whitespace().collect();
    let body = body.strip_prefix(PREFIX).ok_or_else(invalid)?;
    let bytes = URL_SAFE_NO_PAD.decode(body).map_err(|_| invalid())?;
    if bytes.len() != 7 + 64 {
        return Err(invalid());
    }
    let (payload, sig) = bytes.split_at(7);
    let sig: [u8; 64] = sig.try_into().map_err(|_| invalid())?;
    let public = VerifyingKey::from_bytes(public).map_err(|_| invalid())?;
    public.verify_strict(&[CONTEXT, payload].concat(), &Signature::from_bytes(&sig)).map_err(|_| invalid())?;
    if payload[0] != VERSION {
        return Err(anyhow!("This license key needs a newer version of Aonin."));
    }
    let id = payload[1..5].iter().map(|b| format!("{b:02X}")).collect();
    let day = u16::from_be_bytes([payload[5], payload[6]]);
    Ok(License { id, updates_until: civil_from_days(EPOCH_DAYS + day as i64) })
}

/// Whether `license` covers a build released on `release_date` (YYYY-MM-DD).
pub fn covers(license: &License, release_date: &str) -> bool {
    release_date <= license.updates_until.as_str()
}

// Calendar dates without a date crate (Howard Hinnant's algorithms).
const fn days_from_civil(y: i64, m: i64, d: i64) -> i64 {
    let y = if m <= 2 { y - 1 } else { y };
    let era = if y >= 0 { y } else { y - 399 } / 400;
    let yoe = y - era * 400;
    let mp = (m + 9) % 12;
    let doy = (153 * mp + 2) / 5 + d - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    era * 146_097 + doe - 719_468
}

fn civil_from_days(z: i64) -> String {
    let z = z + 719_468;
    let era = if z >= 0 { z } else { z - 146_096 } / 146_097;
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = yoe + era * 400 + if m <= 2 { 1 } else { 0 };
    format!("{y:04}-{m:02}-{d:02}")
}

#[cfg(test)]
mod tests {
    use super::*;
    use ed25519_dalek::{Signer, SigningKey};

    fn key_for(signer: &SigningKey, payload: [u8; 7]) -> String {
        let sig = signer.sign(&[CONTEXT, &payload].concat());
        format!("{PREFIX}{}", URL_SAFE_NO_PAD.encode([&payload[..], &sig.to_bytes()].concat()))
    }

    /// id A1B2C3D4, updates until day 672 = 2027-11-04.
    const PAYLOAD: [u8; 7] = [2, 0xA1, 0xB2, 0xC3, 0xD4, 0x02, 0xA0];

    #[test]
    fn genuine_key_is_accepted() {
        let signer = SigningKey::from_bytes(&[7; 32]);
        let key = key_for(&signer, PAYLOAD);
        assert!(key.len() <= 101, "{} characters", key.len());
        let l = verify_with(&key, signer.verifying_key().as_bytes()).unwrap();
        assert_eq!(l, License { id: "A1B2C3D4".into(), updates_until: "2027-11-04".into() });
        // Pasted with line breaks or spaces still works.
        let wrapped = format!("  {}\n{}  ", &key[..30], &key[30..]);
        assert!(verify_with(&wrapped, signer.verifying_key().as_bytes()).is_ok());
    }

    #[test]
    fn changed_or_foreign_keys_are_refused() {
        let signer = SigningKey::from_bytes(&[7; 32]);
        let other = SigningKey::from_bytes(&[8; 32]);
        assert!(verify_with(&key_for(&other, PAYLOAD), signer.verifying_key().as_bytes()).is_err());
        // A later date put in, old signature kept.
        let key = key_for(&signer, PAYLOAD);
        let mut bytes = URL_SAFE_NO_PAD.decode(&key[PREFIX.len()..]).unwrap();
        bytes[6] = 0xFF;
        let forged = format!("{PREFIX}{}", URL_SAFE_NO_PAD.encode(bytes));
        assert!(verify_with(&forged, signer.verifying_key().as_bytes()).is_err());
        assert!(verify("AONIN-nonsense").is_err());
        assert!(verify("").is_err());
    }

    #[test]
    fn dates() {
        assert_eq!(civil_from_days(EPOCH_DAYS), "2026-01-01");
        assert_eq!(civil_from_days(EPOCH_DAYS + 672), "2027-11-04");
        assert_eq!(civil_from_days(EPOCH_DAYS + 790), "2028-03-01", "through a leap day");
        let l = License { id: "x".into(), updates_until: "2027-11-04".into() };
        assert!(covers(&l, "2026-12-01"));
        assert!(covers(&l, "2027-11-04"));
        assert!(!covers(&l, "2027-11-05"));
    }
}
