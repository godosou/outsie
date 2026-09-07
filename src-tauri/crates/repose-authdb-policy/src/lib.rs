//! Pure transformations for Repose's supported macOS authorization policy.
//!
//! This crate deliberately has no API for selecting an authorization right. It
//! only understands the already-logged-in-session unlock right,
//! `system.login.screensaver`, and never reads or writes the live policy store.

mod transform;

pub use transform::{PolicyError, PolicySpec, ScreenSaverPolicy};
