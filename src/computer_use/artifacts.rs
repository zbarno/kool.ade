//! Controller-mediated private staging for task, guest, operator, and clipboard data.
//!
//! The bridge stores bytes only in the installation-local operational store.
//! It does not expose host paths to a guest or publish artifacts to Git.

mod contract;
mod store;
#[cfg(test)]
mod tests;
mod validation;

pub use contract::{
    COMPUTER_ARTIFACT_MAX_BYTES_V1, COMPUTER_ARTIFACT_STORE_QUOTA_BYTES_V1,
    COMPUTER_CLIPBOARD_MAX_BYTES_V1, ComputerArtifactFlowV1, ComputerArtifactManifestV1,
    ComputerArtifactReceiptV1, ComputerClipboardDirectionV1, ComputerClipboardGrantV1,
    ComputerTransferGrantRequestV1, ComputerTransferGrantV1,
};
pub use store::{ComputerArtifactBridgeErrorV1, ComputerArtifactBridgeV1};
