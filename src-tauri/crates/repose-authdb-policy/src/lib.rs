//! Pure transformations for Repose's supported macOS authorization policy.
//!
//! This crate deliberately has no API for selecting an authorization right. It
//! only understands the already-logged-in-session unlock right,
//! `system.login.screensaver`, and never reads or writes the live policy store.
//! A plist does not carry the name of the right it came from, so callers must
//! only supply and write back bytes through the later fixed-right screensaver
//! store adapter. That adapter must not accept an arbitrary right name.
//!
//! Parsing accepts at most 1 MiB of encoded input, 64 nested collections,
//! 16,384 expanded plist events, and 256 KiB of cumulative expanded scalar,
//! key, string, and data bytes. The expansion limits matter for binary plists:
//! their object graph may reuse one object many times even when the file itself
//! is small. Before the generic plist reader runs, binary input must also use a
//! contiguous, acyclic layout in which every declared object is reachable from
//! the root. Preflight completes before the value tree is materialized.

mod binary_layout;
mod transform;

pub use transform::{PolicyError, PolicySpec, ScreenSaverPolicy};
