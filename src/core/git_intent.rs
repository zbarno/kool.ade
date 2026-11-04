use serde::{Deserialize, Serialize};

const VERSION: u32 = 1;

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum PublicationModeV1 {
    #[default]
    HoldForReview,
    CreatePullRequest,
    AutoPublish,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GitIntentSelectionV1 {
    pub source_ref: String,
    pub destination_ref: String,
    pub publication_mode: PublicationModeV1,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct GitIntentV1 {
    pub schema_version: u32,
    pub repository_id: String,
    pub source_ref: String,
    pub source_sha: String,
    pub destination_ref: String,
    pub publication_mode: PublicationModeV1,
    pub approval_revision: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub inherited_from_task_uid: Option<String>,
}

impl GitIntentV1 {
    pub fn approve(
        repository_id: String,
        source_ref: String,
        source_sha: String,
        destination_ref: String,
        publication_mode: PublicationModeV1,
        inherited_from_task_uid: Option<String>,
    ) -> anyhow::Result<Self> {
        let mut intent = Self {
            schema_version: VERSION,
            repository_id,
            source_ref,
            source_sha,
            destination_ref,
            publication_mode,
            approval_revision: String::new(),
            inherited_from_task_uid,
        };
        intent.validate_fields()?;
        intent.approval_revision = intent.calculate_revision();
        Ok(intent)
    }

    pub fn validate(&self) -> anyhow::Result<()> {
        self.validate_fields()?;
        anyhow::ensure!(
            self.approval_revision == self.calculate_revision(),
            "Git intent approval revision does not match its repository, refs, source SHA, publication mode, and inheritance"
        );
        Ok(())
    }

    pub fn inherited_by(&self, task_uid: String) -> anyhow::Result<Self> {
        self.validate()?;
        Self::approve(
            self.repository_id.clone(),
            self.source_ref.clone(),
            self.source_sha.clone(),
            self.destination_ref.clone(),
            self.publication_mode,
            Some(task_uid),
        )
    }

    fn validate_fields(&self) -> anyhow::Result<()> {
        anyhow::ensure!(
            self.schema_version == VERSION,
            "Unsupported Git intent schema version {}",
            self.schema_version
        );
        anyhow::ensure!(
            valid_repository_id(&self.repository_id),
            "Invalid Git intent repository ID"
        );
        validate_ref(&self.source_ref, "source")?;
        validate_ref(&self.destination_ref, "destination")?;
        anyhow::ensure!(
            matches!(self.source_sha.len(), 40 | 64)
                && self.source_sha.bytes().all(|byte| byte.is_ascii_hexdigit()),
            "Invalid Git intent source SHA"
        );
        if let Some(uid) = self.inherited_from_task_uid.as_deref() {
            anyhow::ensure!(
                uuid::Uuid::parse_str(uid).is_ok(),
                "Invalid inherited Git intent task UID"
            );
        }
        Ok(())
    }

    fn calculate_revision(&self) -> String {
        use sha2::Digest as _;
        let input = serde_json::json!({
            "schemaVersion": self.schema_version,
            "repositoryId": self.repository_id,
            "sourceRef": self.source_ref,
            "sourceSha": self.source_sha,
            "destinationRef": self.destination_ref,
            "publicationMode": self.publication_mode,
            "inheritedFromTaskUid": self.inherited_from_task_uid,
        });
        let digest = sha2::Sha256::digest(serde_json::to_vec(&input).unwrap_or_default());
        let encoded = digest
            .iter()
            .map(|byte| format!("{byte:02x}"))
            .collect::<String>();
        format!("sha256:{encoded}")
    }
}

pub(crate) fn validate_ref(value: &str, kind: &str) -> anyhow::Result<()> {
    anyhow::ensure!(
        !value.is_empty()
            && value.len() <= 255
            && !value.starts_with('-')
            && !value.starts_with('/')
            && !value.ends_with('/')
            && !value.ends_with('.')
            && !value.contains("..")
            && !value.contains("//")
            && !value.contains("@{")
            && value != "@"
            && !value
                .chars()
                .any(|ch| ch.is_ascii_control() || ch.is_whitespace() || "~^:?*[\\".contains(ch))
            && value
                .split('/')
                .all(|component| !component.starts_with('.') && !component.ends_with(".lock")),
        "Invalid Git intent {kind} ref"
    );
    Ok(())
}

fn valid_repository_id(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 40
        && value
            .bytes()
            .all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || byte == b'-')
}

#[cfg(test)]
#[path = "git_intent_tests.rs"]
mod tests;
