use std::path::Path;

use crate::execution::contract::ReadinessState;

/// Keep Pi authentication unknown without running its credential resolver.
/// Pi permits `auth.json` keys to invoke a configured host command, so even a
/// non-generative `auth check` could execute user-defined host code.
pub(super) fn pi_authentication(_binary: &Path, _provider: Option<&str>) -> ReadinessState {
    ReadinessState::Unknown
}

#[cfg(all(test, unix))]
#[path = "authentication/tests.rs"]
mod tests;
