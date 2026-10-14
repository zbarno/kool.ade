//! Versioned, provider-neutral instructions and typed contracts for agent roles.
mod render;
mod schemas;
#[cfg(test)]
mod tests;
mod types;

pub use render::{PromptManifest, RenderedPrompt, render, validate_prompt_manifest};
pub use schemas::*;
pub(crate) use types::validate_payload;
pub use types::{
    EvidenceConfidentiality, EvidenceItem, EvidenceOrigin, PromptRole, RoleInputV1, RolePayloadV1,
};
