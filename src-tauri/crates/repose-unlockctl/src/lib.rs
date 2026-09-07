//! Only read-only inspection and fail-closed production command facades are
//! public. Native and concrete backend mutation surfaces stay crate-private.
//!
//! ```compile_fail
//! use repose_unlockctl::authdb::AuthorizationDb;
//! ```
//!
//! ```compile_fail
//! use repose_unlockctl::production::ProductionBackend;
//! ```

#![deny(unsafe_code)]

pub mod artifact_verify;
mod authdb;
pub mod cli;
pub mod install_transaction;
#[allow(unsafe_code)]
pub mod production;
