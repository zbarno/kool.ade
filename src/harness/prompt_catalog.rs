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
    BootstrapAttributedTextV1, BootstrapConversationMessageV1, BootstrapConversationPayloadV1,
    BootstrapConversationSourceV1, BootstrapConversationSpeakerV1, BootstrapFactDomainV1,
    BootstrapFactSourceV1, BootstrapFactStatusV1, BootstrapPlatformV1, BootstrapTaskFactV1,
    BootstrapTaskStatusV1, EvidenceConfidentiality, EvidenceItem, EvidenceOrigin, PromptRole,
    RoleInputV1, RolePayloadV1,
};
