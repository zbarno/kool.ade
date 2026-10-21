use crate::execution::contract::{ExecutionTarget, ResourceBudgetV1, WorkspaceRef};

#[derive(Clone, Debug, serde::Deserialize, serde::Serialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct BackendProbeV1 {
    pub wire_version: u16,
    pub available: bool,
    pub compatible: bool,
    #[serde(default)]
    pub architecture: Option<String>,
    pub failure: Option<crate::execution::errors::ExecutionErrorKind>,
    #[serde(default)]
    pub capabilities: Option<BackendCapabilitiesV1>,
}

/// Negotiated worker/runtime facts. These values are observations, not grants;
/// every request is still checked against the local immutable policy.
#[derive(Clone, Debug, serde::Deserialize, serde::Serialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct BackendCapabilitiesV1 {
    pub app_version: String,
    pub docker_version: Option<String>,
    pub docker_api_version: Option<String>,
    pub docker_rootless: Option<bool>,
    pub docker_desktop: Option<bool>,
    pub linux_containers: Option<bool>,
    pub seccomp_enabled: Option<bool>,
    pub memory_limit: Option<bool>,
    pub cpu_quota: Option<bool>,
    pub pids_limit: Option<bool>,
    pub disk_available_bytes: Option<u64>,
    /// CPU capacity remaining after active Kool.ad worker reservations.
    /// This does not measure CPU used by unmanaged host processes.
    #[serde(default)]
    pub available_cpu_millis: Option<u32>,
    /// Linux MemAvailable capped by the worker's cgroup memory limits.
    #[serde(default)]
    pub available_memory_bytes: Option<u64>,
    /// Linux CPU pressure stall information `some avg10`, in basis points.
    /// This is a load preference signal, not CPU capacity or authorization.
    #[serde(default)]
    pub cpu_pressure_some_avg10_basis_points: Option<u32>,
    /// Legacy optional load observation; never treated as millicore capacity.
    #[serde(default)]
    pub load_millis: Option<u32>,
    /// Capability observations; false means this backend cannot provide the
    /// requested host resource to an execution container.
    #[serde(default)]
    pub gpu_available: bool,
    #[serde(default)]
    pub display_available: bool,
    pub current_execution_count: u32,
    #[serde(default)]
    pub maximum_execution_count: Option<u32>,
    pub resource_ceiling: ResourceBudgetV1,
    pub trusted_image_digests: Vec<String>,
    pub policy_hash: String,
}

#[derive(Clone, Debug, serde::Deserialize, serde::Serialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct WorkspaceLeaseV1 {
    pub lease_id: String,
    pub workspace: WorkspaceRef,
    pub expires_unix_ms: u64,
}

#[derive(Clone, Debug, serde::Deserialize, serde::Serialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct ExecutionHandleV1 {
    pub execution_id: String,
    pub task_id: String,
    pub target: ExecutionTarget,
    pub request_sequence: u64,
}
