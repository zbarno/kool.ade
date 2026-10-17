use serde::{Deserialize, Serialize};

/// An immutable disposition for one origin included in a reconciliation batch.
/// Delta references describe the joint result, not independent causal claims
/// that every origin introduced every referenced requirement change.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct SpecOriginDispositionV1 {
    pub origin_event_uid: String,
    pub outcome: SpecOriginOutcomeV1,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum SpecOriginOutcomeV1 {
    DeltaReferences {
        deltas: Vec<SpecDeltaReferenceV1>,
    },
    NoChange {
        reason: String,
    },
    /// The source remains pending; this is not a successful no-change result.
    Unresolved {
        reason: String,
    },
}

/// Resolves within the same immutable specification revision as the outcome.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct SpecDeltaReferenceV1 {
    pub event_uid: String,
    pub requirement_id: String,
}
