use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, Deserialize, Serialize, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[serde(rename_all = "snake_case")]
pub enum DependencyCacheEcosystemV1 {
    Npm,
    Cargo,
}

/// Opaque authorization for an immutable broker-verified cache snapshot.
/// It contains no host path or Docker resource name.
#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[serde(deny_unknown_fields)]
pub struct DependencyCacheRefV1 {
    pub ecosystem: DependencyCacheEcosystemV1,
    pub task_id: String,
    pub project_id: String,
    pub resolved_integrity: String,
    pub target_arch: String,
    pub target_id: String,
    pub manifest_sha256: String,
    pub size_bytes: u64,
}

impl DependencyCacheRefV1 {
    pub fn validate(&self) -> anyhow::Result<()> {
        for value in [&self.task_id, &self.project_id] {
            super::super::ids::validate_identifier(value)?;
        }
        anyhow::ensure!(
            matches!(
                self.target_arch.as_str(),
                "x86_64" | "aarch64" | "amd64" | "arm64"
            ),
            "dependency cache target architecture is unsupported"
        );
        anyhow::ensure!(
            self.target_id == "local"
                || super::super::ids::validate_identifier(&self.target_id).is_ok(),
            "dependency cache target identity is invalid"
        );
        anyhow::ensure!(
            !self.resolved_integrity.is_empty()
                && self.resolved_integrity.len() <= 4096
                && !self
                    .resolved_integrity
                    .bytes()
                    .any(|byte| byte.is_ascii_control()),
            "dependency cache integrity identity is invalid"
        );
        anyhow::ensure!(
            self.manifest_sha256.len() == 64
                && self
                    .manifest_sha256
                    .bytes()
                    .all(|byte| byte.is_ascii_hexdigit()),
            "dependency cache manifest digest is invalid"
        );
        anyhow::ensure!(
            (1..=128 * 1024 * 1024).contains(&self.size_bytes),
            "dependency cache snapshot size is outside supported limits"
        );
        Ok(())
    }
}
