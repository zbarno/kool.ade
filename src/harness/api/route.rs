use crate::execution::contract::HarnessRole;
use serde::{Deserialize, Serialize};

/// Immutable routing evidence attached to an adapter when a task turn starts.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct HarnessRouteProvenance {
    pub provider_id: String,
    pub role: HarnessRole,
    pub work_category: Option<String>,
    /// Model pinned for this invocation, including a refreshed provider default.
    #[serde(default)]
    pub selected_model: Option<String>,
    /// Model explicitly chosen in the saved route, when present.
    pub requested_model: Option<String>,
    pub capability_probe_id: Option<String>,
    pub capability_generation: Option<u64>,
}
