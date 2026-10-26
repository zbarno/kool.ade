use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct EngineSnapshot {
    pub daemon_id: String,
    pub daemon_version: String,
    pub api_version: String,
    pub minimum_api_version: String,
    pub os_type: String,
    pub architecture: String,
    pub security_options: Vec<String>,
    pub rootless: bool,
    pub native_linux: bool,
    pub docker_desktop: bool,
    pub memory_limit: bool,
    pub cpu_quota: bool,
    pub pids_limit: bool,
    #[serde(default)]
    pub cgroup_version: Option<String>,
    #[serde(default)]
    pub cgroup_driver: Option<String>,
    pub seccomp_enabled: bool,
    pub docker_storage_accessible: bool,
    pub disk_available_bytes: Option<u64>,
    pub minimum_disk_bytes: u64,
}
