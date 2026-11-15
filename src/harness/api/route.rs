use crate::execution::contract::{AuthenticationModeV1, HarnessRole};
use serde::{Deserialize, Serialize};

/// Route identity that can safely be persisted while capability evidence is refreshed.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct CapabilityRefreshBindingV1 {
    pub provider_id: String,
    pub role: crate::execution::contract::HarnessRole,
    pub work_category: String,
    pub selected_model: String,
    pub requested_model: Option<String>,
    #[serde(default)]
    pub observed_at_unix_ms: u64,
}

impl CapabilityRefreshBindingV1 {
    pub fn is_valid(&self) -> bool {
        let bounded = |value: &str, max| !value.trim().is_empty() && value.len() <= max;
        bounded(&self.provider_id, 128)
            && bounded(&self.work_category, 128)
            && bounded(&self.selected_model, 512)
            && self
                .requested_model
                .as_deref()
                .is_none_or(|model| bounded(model, 512))
    }

    pub fn same_route(&self, other: &Self) -> bool {
        self.provider_id == other.provider_id
            && self.role == other.role
            && self.work_category == other.work_category
            && self.selected_model == other.selected_model
            && self.requested_model == other.requested_model
    }
}

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
    /// Auth source class bound to the role profile at discovery time.
    #[serde(default)]
    pub authentication_mode: Option<AuthenticationModeV1>,
    /// Exact version/auth/profile identity authorized by discovery.
    #[serde(default)]
    pub profile_evidence_id: Option<String>,
    /// Exact CLI version attached to the role/profile discovery result.
    #[serde(default)]
    pub cli_version: Option<String>,
    /// Discovery evidence expiry; routed invocation refuses to use it past this time.
    #[serde(default)]
    pub capability_expires_at_unix_ms: Option<u64>,
}
