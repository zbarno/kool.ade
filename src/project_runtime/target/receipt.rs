use serde::{Deserialize, Serialize};

use super::{RuntimeOperation, RuntimeReceiptState};
use crate::project_runtime::recipe::{valid_id, validate_relative_path};

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct RuntimeReceiptV1 {
    pub schema_version: u16,
    pub receipt_id: String,
    pub operation: RuntimeOperation,
    pub task_id: String,
    pub target_id: String,
    pub source_sha: String,
    pub recipe_id: Option<String>,
    pub state: RuntimeReceiptState,
    pub exit_code: Option<i32>,
    pub artifact_hashes: std::collections::BTreeMap<String, String>,
    /// Target lockfile dependencies authorized and staged before this recipe ran.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub prepared_dependency_ids: Vec<String>,
    pub log_reference: Option<String>,
    pub observed_at_unix_ms: u64,
}

impl RuntimeReceiptV1 {
    pub fn validate(&self) -> Result<(), String> {
        let valid_sha = |value: &str| {
            matches!(value.len(), 40 | 64) && value.bytes().all(|byte| byte.is_ascii_hexdigit())
        };
        if self.schema_version != 1
            || !valid_id(&self.receipt_id)
            || !valid_id(&self.task_id)
            || !valid_id(&self.target_id)
            || !valid_sha(&self.source_sha)
            || self.artifact_hashes.len() > 256
            || self.prepared_dependency_ids.len() > 128
        {
            return Err("runtime receipt is invalid or exceeds limits".into());
        }
        let mut prepared_ids = std::collections::BTreeSet::new();
        if self
            .prepared_dependency_ids
            .iter()
            .any(|id| !super::manifest::valid_dependency_id(id) || !prepared_ids.insert(id))
        {
            return Err(
                "runtime receipt contains invalid or duplicate prepared dependencies".into(),
            );
        }
        if self.state == RuntimeReceiptState::Passed && self.exit_code != Some(0) {
            return Err("a successful runtime receipt must have observed exit code zero".into());
        }
        if self.state == RuntimeReceiptState::Running && self.exit_code.is_some() {
            return Err("a running runtime receipt cannot have a terminal exit code".into());
        }
        if self.recipe_id.as_ref().is_some_and(|id| !valid_id(id))
            || self.log_reference.as_ref().is_some_and(|reference| {
                reference.is_empty()
                    || reference.len() > 256
                    || reference.contains('/')
                    || reference.contains('\\')
            })
        {
            return Err("runtime receipt contains an invalid recipe or log reference".into());
        }
        for (path, digest) in &self.artifact_hashes {
            validate_relative_path(path)?;
            if digest.len() != 64 || !digest.bytes().all(|byte| byte.is_ascii_hexdigit()) {
                return Err("runtime artifact digest is invalid".into());
            }
        }
        Ok(())
    }
}
