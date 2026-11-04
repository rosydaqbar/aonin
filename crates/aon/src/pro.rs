//! Where the free engine ends and Aonin Pro begins.
//!
//! The engine asks a `Pro` before doing anything that Pro unlocks. The free
//! build passes `Free`, which says no and has no listener-side private
//! session code at all. The Pro build passes its own implementation from
//! the `aon-pro` crate.
//!
//! Everything a free device needs to take part stays here: joining a
//! private listener with a code, being carried along when a listener turns
//! private, and receiving a handover.

use crate::license::License;
use crate::private::Cipher;
use anyhow::{Result, anyhow};
use serde::Serialize;
use std::sync::Arc;

#[derive(Serialize, Clone, Copy, PartialEq, Eq, Debug)]
#[serde(rename_all = "lowercase")]
pub enum Edition {
    Free,
    Pro,
}

/// What Pro unlocks on this device.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Feature {
    /// Turn this device's listener private (show a code).
    MakePrivate,
    /// Hand the listener role to another device.
    HandOver,
}

pub trait Pro: Send + Sync {
    fn edition(&self) -> Edition;
    /// The license entered on this device, checked already (see `license`).
    fn set_license(&self, license: Option<License>);
    /// `Ok` when this device may use `feature`; the error says why not.
    fn allow(&self, feature: Feature) -> Result<()>;
    /// The listener side of a private session. `None` without Pro: the
    /// listener then stays private but can't let anyone in.
    fn private_hub(&self) -> Option<Arc<dyn PrivateHubOps>>;
}

/// Listener side of a private session (see `private` for the packets).
pub trait PrivateHubOps: Send + Sync {
    /// Answer a PairStart with the code on screen: the reply to send and the
    /// key for that sender (one it doesn't have if its code was wrong).
    fn answer(&self, start: &[u8], code: &str, name: &str) -> Option<(Vec<u8>, Cipher)>;
    /// Answer an UpgradeRequest from a device that was connected when the
    /// listener turned private.
    fn answer_upgrade(&self, request: &[u8], code: &str) -> Option<Vec<u8>>;
}

/// The free edition.
pub struct Free;

impl Pro for Free {
    fn edition(&self) -> Edition {
        Edition::Free
    }

    /// The free build only needs a license to fetch Aonin Pro.
    fn set_license(&self, _license: Option<License>) {}

    fn allow(&self, feature: Feature) -> Result<()> {
        Err(needs_pro(feature))
    }

    fn private_hub(&self) -> Option<Arc<dyn PrivateHubOps>> {
        None
    }
}

/// The message shown when `feature` is locked.
pub fn needs_pro(feature: Feature) -> anyhow::Error {
    match feature {
        Feature::MakePrivate => anyhow!("Making a listener private needs Aonin Pro."),
        Feature::HandOver => anyhow!("Make listener needs Aonin Pro."),
    }
}
