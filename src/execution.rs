//! Versioned contracts for the future Docker-backed local and remote executors.
//! This module defines interfaces only; it does not start containers.

pub mod backend;
pub mod contract;
pub mod docker;
pub mod errors;
pub mod policy;
pub mod remote;
pub(crate) mod resources;
pub mod session;

#[cfg(test)]
#[path = "execution/tests.rs"]
mod tests;
