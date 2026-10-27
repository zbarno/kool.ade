mod controller;
pub(super) mod recipe;
pub(super) mod verifier;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum ClaudeInstallProgress {
    Download,
    TrustVerification,
    CurrentUserInstall,
    FreshProviderProbe,
}

pub(crate) use controller::{ClaudeInstallController, ClaudeInstallStatus};
