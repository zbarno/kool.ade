use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use crate::execution::{
    contract::{ExecutionResultV1, WorkspaceRef},
    remote::WorkspaceManifestV1,
};

pub const MAX_TRANSFER_FILES: usize = 100_000;

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct TransferManifestV1 {
    pub schema_version: u16,
    pub transfer_id: String,
    /// Authenticated sender identity: `controller:<id>` or `worker:<id>`.
    pub source_identity: String,
    /// Authenticated recipient identity in the same namespace.
    pub destination_identity: String,
    pub payload: TransferPayloadV1,
    pub payload_sha256: String,
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(tag = "kind", content = "value", rename_all = "snake_case")]
pub enum TransferPayloadV1 {
    Workspace(WorkspaceManifestV1),
    WorkspaceResult(WorkspaceManifestV1),
    Results(ExecutionResultV1),
}

impl TransferManifestV1 {
    pub fn workspace(
        source_identity: impl Into<String>,
        destination_identity: impl Into<String>,
        workspace: WorkspaceManifestV1,
    ) -> anyhow::Result<Self> {
        Self::new(
            source_identity.into(),
            destination_identity.into(),
            TransferPayloadV1::Workspace(workspace),
        )
    }

    pub fn results(
        source_identity: impl Into<String>,
        destination_identity: impl Into<String>,
        result: ExecutionResultV1,
    ) -> anyhow::Result<Self> {
        Self::new(
            source_identity.into(),
            destination_identity.into(),
            TransferPayloadV1::Results(result),
        )
    }

    pub fn workspace_result(
        source_identity: impl Into<String>,
        destination_identity: impl Into<String>,
        workspace: WorkspaceManifestV1,
    ) -> anyhow::Result<Self> {
        Self::new(
            source_identity.into(),
            destination_identity.into(),
            TransferPayloadV1::WorkspaceResult(workspace),
        )
    }

    fn new(
        source_identity: String,
        destination_identity: String,
        payload: TransferPayloadV1,
    ) -> anyhow::Result<Self> {
        let payload_sha256 = digest(&payload)?;
        let manifest = Self {
            schema_version: 1,
            transfer_id: uuid::Uuid::new_v4().hyphenated().to_string(),
            source_identity,
            destination_identity,
            payload,
            payload_sha256,
        };
        manifest.validate()?;
        Ok(manifest)
    }

    pub fn validate(&self) -> anyhow::Result<()> {
        anyhow::ensure!(
            self.schema_version == 1,
            "unsupported transfer manifest version"
        );
        let id = uuid::Uuid::parse_str(&self.transfer_id)?;
        anyhow::ensure!(
            id.hyphenated().to_string() == self.transfer_id,
            "transfer ID is not canonical"
        );
        anyhow::ensure!(
            valid_identity(&self.source_identity) && valid_identity(&self.destination_identity),
            "transfer identity is invalid"
        );
        anyhow::ensure!(
            self.source_identity != self.destination_identity,
            "transfer source and destination are equal"
        );
        anyhow::ensure!(
            digest(&self.payload)? == self.payload_sha256,
            "transfer payload digest does not match"
        );
        let files = match &self.payload {
            TransferPayloadV1::Workspace(manifest) => {
                manifest.validate()?;
                anyhow::ensure!(
                    self.source_identity.starts_with("controller:")
                        && self.destination_identity.starts_with("worker:"),
                    "workspace transfer direction is invalid"
                );
                manifest
                    .entries
                    .iter()
                    .filter(|entry| {
                        entry.kind == crate::execution::remote::WorkspaceEntryKindV1::File
                    })
                    .count()
            }
            TransferPayloadV1::WorkspaceResult(manifest) => {
                manifest.validate()?;
                anyhow::ensure!(
                    self.source_identity.starts_with("worker:")
                        && self.destination_identity.starts_with("controller:"),
                    "workspace result transfer direction is invalid"
                );
                manifest
                    .entries
                    .iter()
                    .filter(|entry| {
                        entry.kind == crate::execution::remote::WorkspaceEntryKindV1::File
                    })
                    .count()
            }
            TransferPayloadV1::Results(result) => {
                result.validate()?;
                anyhow::ensure!(
                    self.source_identity.starts_with("worker:")
                        && self.destination_identity.starts_with("controller:"),
                    "result transfer direction is invalid"
                );
                for artifact in &result.artifacts {
                    validate_transfer_path(&artifact.relative_path)?;
                }
                result.artifacts.len()
            }
        };
        let file_refs = self.files();
        let mut names = std::collections::HashSet::new();
        for file in &file_refs {
            validate_transfer_path(file.path)?;
            anyhow::ensure!(
                names.insert(file.path.to_lowercase()),
                "transfer contains case-insensitive file collisions"
            );
        }
        for file in &file_refs {
            let mut ancestor = std::path::Path::new(file.path).parent();
            while let Some(path) = ancestor {
                if path.as_os_str().is_empty() {
                    break;
                }
                let ancestor_name = path.to_string_lossy().to_lowercase();
                anyhow::ensure!(
                    !names.contains(&ancestor_name),
                    "transfer file is nested beneath another file"
                );
                ancestor = path.parent();
            }
        }
        anyhow::ensure!(
            files <= MAX_TRANSFER_FILES,
            "transfer manifest has too many files"
        );
        anyhow::ensure!(
            serde_json::to_vec(self)?.len() <= 512 * 1024,
            "transfer manifest exceeds its size limit"
        );
        Ok(())
    }

    pub fn workspace_ref(&self) -> Option<&WorkspaceRef> {
        match &self.payload {
            TransferPayloadV1::Workspace(manifest) => Some(&manifest.workspace),
            TransferPayloadV1::WorkspaceResult(manifest) => Some(&manifest.workspace),
            TransferPayloadV1::Results(_) => None,
        }
    }

    pub fn is_workspace_result(&self) -> bool {
        matches!(self.payload, TransferPayloadV1::WorkspaceResult(_))
    }

    /// File paths and declared sizes for authenticated streaming clients.
    pub fn file_specs(&self) -> Vec<(&str, u64)> {
        self.files()
            .into_iter()
            .map(|file| (file.path, file.size_bytes))
            .collect()
    }

    pub(super) fn files(&self) -> Vec<TransferFileRef<'_>> {
        match &self.payload {
            TransferPayloadV1::Workspace(manifest)
            | TransferPayloadV1::WorkspaceResult(manifest) => manifest
                .entries
                .iter()
                .filter_map(|entry| {
                    (entry.kind == crate::execution::remote::WorkspaceEntryKindV1::File).then_some(
                        TransferFileRef {
                            path: &entry.path,
                            size_bytes: entry.size_bytes,
                            sha256: entry.sha256.as_deref().unwrap_or_default(),
                            executable: entry.executable,
                        },
                    )
                })
                .collect(),
            TransferPayloadV1::Results(result) => result
                .artifacts
                .iter()
                .map(|entry| TransferFileRef {
                    path: &entry.relative_path,
                    size_bytes: entry.size_bytes,
                    sha256: &entry.sha256,
                    executable: false,
                })
                .collect(),
        }
    }
}

pub(super) struct TransferFileRef<'a> {
    pub path: &'a str,
    pub size_bytes: u64,
    pub sha256: &'a str,
    pub executable: bool,
}

fn digest(payload: &TransferPayloadV1) -> anyhow::Result<String> {
    Ok(format!(
        "{:x}",
        Sha256::digest(serde_json::to_vec(payload)?)
    ))
}

fn valid_identity(identity: &str) -> bool {
    let Some((kind, id)) = identity.split_once(':') else {
        return false;
    };
    matches!(kind, "controller" | "worker")
        && !id.is_empty()
        && id.len() <= 128
        && id
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_'))
}

pub(super) fn validate_transfer_path(path: &str) -> anyhow::Result<()> {
    anyhow::ensure!(
        !path.is_empty()
            && !path.starts_with('/')
            && !path.contains('\\')
            && !path.contains(':')
            && !path.chars().any(char::is_control)
            && path.len() <= 4096
            && path.split('/').count() <= 128
            && path.split('/').all(valid_component),
        "transfer path is not normalized"
    );
    Ok(())
}

fn valid_component(component: &str) -> bool {
    if component.is_empty()
        || component == "."
        || component == ".."
        || component.ends_with('.')
        || component.ends_with(' ')
        || component
            .chars()
            .any(|character| "*?\"<>|".contains(character))
    {
        return false;
    }
    let basename = component
        .split('.')
        .next()
        .unwrap_or_default()
        .to_ascii_uppercase();
    !matches!(basename.as_str(), "CON" | "PRN" | "AUX" | "NUL")
        && !(basename.len() == 4
            && (basename.starts_with("COM") || basename.starts_with("LPT"))
            && matches!(basename.as_bytes()[3], b'1'..=b'9'))
}
