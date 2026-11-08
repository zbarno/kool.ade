//! Typed, bounded read-only investigations over revision-pinned repository evidence.
//!
//! This broker consumes the existing repository profile. It does not grant a
//! model authority to choose host commands, network locations, or unindexed paths.
mod execute;
mod helpers;
mod types;

pub use execute::{InvestigationBroker, InvestigationContext};
pub use types::{
    InvestigationAuthorizationV1, InvestigationBudgetV1, InvestigationEvidenceEnvelopeV1,
    InvestigationOutcomeV1, InvestigationReceiptV1, InvestigationRequestV1, InvestigationScopeV1,
    InvestigationSourceRefV1,
};

#[cfg(test)]
#[path = "investigation_broker/tests.rs"]
mod tests;
