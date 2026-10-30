use std::{fs, path::Path};

use serde::{Deserialize, Serialize};

pub(crate) const MAX_MANIFEST_BYTES: usize = 64 * 1024;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct WorkspaceManifest {
    pub schema_version: u32,
    #[serde(alias = "projectId", with = "canonical_uuid")]
    pub workspace_id: uuid::Uuid,
    pub kind: String,
    pub canonical_ref: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub title: Option<String>,
}

impl WorkspaceManifest {
    pub fn new(workspace_id: uuid::Uuid) -> Self {
        Self {
            title: None,
            schema_version: 1,
            workspace_id,
            kind: "koolade-planning".into(),
            canonical_ref: "refs/heads/main".into(),
        }
    }

    pub fn validate(&self, planning_repo: &Path) -> anyhow::Result<()> {
        if let Some(title) = &self.title {
            anyhow::ensure!(
                !title.trim().is_empty()
                    && title.len() <= 512
                    && !title.chars().any(char::is_control),
                "invalid workspace title"
            );
        }
        anyhow::ensure!(
            self.schema_version == 1,
            "unsupported planning manifest schema"
        );
        anyhow::ensure!(
            self.kind == "koolade-planning",
            "planning manifest kind is not koolade-planning"
        );
        anyhow::ensure!(
            !self.workspace_id.is_nil(),
            "planning manifest workspace ID must not be nil"
        );
        anyhow::ensure!(
            self.canonical_ref.starts_with("refs/heads/"),
            "canonicalRef must name a local planning branch"
        );
        let check = super::git::output(None, &["check-ref-format", &self.canonical_ref])?;
        anyhow::ensure!(
            check.status.success(),
            "planning manifest canonicalRef is not a valid Git ref"
        );
        let exists = super::git::output(
            Some(planning_repo),
            &["show-ref", "--verify", "--quiet", &self.canonical_ref],
        )?;
        anyhow::ensure!(
            exists.status.success(),
            "planning canonical branch {} is missing",
            self.canonical_ref
        );
        Ok(())
    }

    pub fn read(planning_repo: &Path) -> anyhow::Result<Self> {
        let manifest = Self::read_unvalidated(planning_repo)?;
        manifest.validate(planning_repo)?;
        Ok(manifest)
    }

    /// Read bounded, regular-file metadata before a remote canonical branch
    /// can be fetched. Callers must validate the manifest after checkout.
    pub(super) fn read_unvalidated(planning_repo: &Path) -> anyhow::Result<Self> {
        let bytes = read_manifest_bytes(&planning_repo.join("manifest.json"))?;
        Ok(serde_json::from_slice(&bytes)?)
    }

    /// Detect a managed planning checkout without mistaking an unrelated
    /// repository's `manifest.json` for Kool.ad/e workspace metadata.
    pub fn is_managed_repository(planning_repo: &Path) -> anyhow::Result<bool> {
        let manifest_path = planning_repo.join("manifest.json");
        let has_planning_layout = planning_repo
            .join(crate::artifacts::planning_store::paths::PROJECT_MANIFEST)
            .is_file()
            && planning_repo.join("planning").is_dir();
        if !manifest_path.exists() {
            anyhow::ensure!(
                !has_planning_layout,
                "managed planning repository is missing manifest.json"
            );
            return Ok(false);
        }

        let bytes = read_manifest_bytes(&manifest_path)?;
        let value: serde_json::Value = match serde_json::from_slice(&bytes) {
            Ok(value) => value,
            Err(error) if has_planning_layout => {
                return Err(error.into());
            }
            Err(_) => return Ok(false),
        };
        if value.get("kind").and_then(serde_json::Value::as_str) == Some("koolade-planning") {
            Self::read(planning_repo)?;
            return Ok(true);
        }
        anyhow::ensure!(
            !has_planning_layout,
            "planning repository manifest kind is not koolade-planning"
        );
        Ok(false)
    }
}

fn read_manifest_bytes(path: &Path) -> anyhow::Result<Vec<u8>> {
    let metadata = fs::symlink_metadata(path)?;
    anyhow::ensure!(
        metadata.is_file() && !metadata.file_type().is_symlink(),
        "planning manifest must be a regular file"
    );
    anyhow::ensure!(
        metadata.len() <= MAX_MANIFEST_BYTES as u64,
        "planning manifest exceeds the 64 KiB size limit"
    );
    let bytes = fs::read(path)?;
    anyhow::ensure!(
        bytes.len() <= MAX_MANIFEST_BYTES,
        "planning manifest exceeds the 64 KiB size limit"
    );
    Ok(bytes)
}

pub(crate) mod canonical_uuid {
    use serde::{Deserialize, Deserializer, Serializer, de::Error};

    pub fn serialize<S>(value: &uuid::Uuid, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        serializer.serialize_str(&value.hyphenated().to_string())
    }

    pub fn deserialize<'de, D>(deserializer: D) -> Result<uuid::Uuid, D::Error>
    where
        D: Deserializer<'de>,
    {
        let text = String::deserialize(deserializer)?;
        let id = uuid::Uuid::parse_str(&text).map_err(D::Error::custom)?;
        if id.hyphenated().to_string() != text {
            return Err(D::Error::custom(
                "workspace UUID must use canonical lowercase hyphenated form",
            ));
        }
        Ok(id)
    }
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use super::read_manifest_bytes;

    fn test_dir() -> PathBuf {
        let path =
            std::env::temp_dir().join(format!("koolade-manifest-test-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&path).unwrap();
        path
    }

    #[test]
    fn manifest_reader_rejects_oversized_files() {
        let temp = test_dir();
        let path = temp.join("manifest.json");
        std::fs::write(&path, vec![b' '; 64 * 1024 + 1]).unwrap();
        assert!(read_manifest_bytes(&path).is_err());
        std::fs::remove_dir_all(temp).unwrap();
    }

    #[cfg(unix)]
    #[test]
    fn manifest_reader_rejects_symlinks() {
        let temp = test_dir();
        let target = temp.join("outside.json");
        let manifest = temp.join("manifest.json");
        std::fs::write(&target, b"{}").unwrap();
        std::os::unix::fs::symlink(&target, &manifest).unwrap();
        assert!(read_manifest_bytes(&manifest).is_err());
        std::fs::remove_dir_all(temp).unwrap();
    }
}
