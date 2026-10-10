mod branches;
mod collect;
mod records;

pub(super) use branches::{branch_conflicts, collect_branch_artifacts};
pub(super) use collect::{collect, collect_optional};
pub(super) use records::{full_inventory_digest, legacy_workspace_id, stable_record_ids};

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ArtifactMapping {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub repository_id: Option<String>,
    pub source_path: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub sha256: Option<String>,
    pub disposition: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub target_path: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum Kind {
    Shared,
    Private,
    Normalized,
    ReplacedManifest,
}

#[derive(Debug, Clone)]
pub(super) struct Artifact {
    pub relative: String,
    pub bytes: Vec<u8>,
    pub sha256: String,
    pub mapping: ArtifactMapping,
    pub kind: Kind,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BranchArtifact {
    #[serde(default)]
    pub repository_id: String,
    pub source_ref: String,
    pub source_commit: String,
    pub source_path: String,
    pub sha256: String,
    pub disposition: String,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub record_ids: Vec<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub target_path: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BranchConflict {
    pub artifact_path: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub record_id: Option<String>,
    pub kind: String,
    pub sources: Vec<BranchArtifact>,
}
