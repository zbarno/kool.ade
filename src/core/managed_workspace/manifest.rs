use std::{fs, path::Path};

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct WorkspaceManifest {
    pub schema_version: u32,
    #[serde(alias = "projectId", with = "canonical_uuid")]
    pub workspace_id: uuid::Uuid,
    pub kind: String,
    pub canonical_ref: String,
}

impl WorkspaceManifest {
    pub fn new(workspace_id: uuid::Uuid) -> Self {
        Self {
            schema_version: 1,
            workspace_id,
            kind: "koolade-planning".into(),
            canonical_ref: "refs/heads/main".into(),
        }
    }

    pub fn validate(&self, planning_repo: &Path) -> anyhow::Result<()> {
        anyhow::ensure!(
            self.schema_version == 1,
            "unsupported planning manifest schema"
        );
        anyhow::ensure!(
            self.kind == "koolade-planning",
            "planning manifest kind is not koolade-planning"
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
        let bytes = fs::read(planning_repo.join("manifest.json"))?;
        let manifest: Self = serde_json::from_slice(&bytes)?;
        manifest.validate(planning_repo)?;
        Ok(manifest)
    }

    /// Detect a managed planning checkout without mistaking an unrelated
    /// repository's `manifest.json` for Kool.ad/e workspace metadata.
    pub fn is_managed_repository(planning_repo: &Path) -> anyhow::Result<bool> {
        let manifest_path = planning_repo.join("manifest.json");
        let has_planning_layout = planning_repo
            .join(crate::artifacts::planning_store::paths::PROJECT_MANIFEST)
            .is_file()
            && planning_repo.join("planning").is_dir();
        if !manifest_path.is_file() {
            anyhow::ensure!(
                !has_planning_layout,
                "managed planning repository is missing manifest.json"
            );
            return Ok(false);
        }

        let bytes = fs::read(&manifest_path)?;
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
