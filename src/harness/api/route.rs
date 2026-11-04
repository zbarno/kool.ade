use crate::execution::contract::{AuthenticationModeV1, HarnessRole};
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
