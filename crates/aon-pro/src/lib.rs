//! The free edition's stand-in for Aonin Pro.
//!
//! The app and the CLI get their `Pro` from `aon_pro::new()`. Here it is
//! `aon::pro::Free`: Pro features say so and the listener side of private
//! sessions isn't included. Official Pro builds put a crate with the same
//! name and API in this folder before building.

use aon::pro::{Free, Pro};
use std::sync::Arc;

pub fn new() -> Arc<dyn Pro> {
    Arc::new(Free)
}
