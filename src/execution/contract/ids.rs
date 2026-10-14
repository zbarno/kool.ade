use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum TargetLocation {
    Local,
    Remote { worker_id: String },
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct ExecutionTarget {
    pub target_id: String,
    pub location: TargetLocation,
}

impl ExecutionTarget {
    pub fn local() -> Self {
        Self {
            target_id: "local".into(),
            location: TargetLocation::Local,
        }
    }

    pub fn remote(worker_id: impl Into<String>) -> Self {
        let worker_id = worker_id.into();
        Self {
            target_id: worker_id.clone(),
            location: TargetLocation::Remote { worker_id },
        }
    }

    pub fn validate(&self) -> anyhow::Result<()> {
        match &self.location {
            TargetLocation::Local => {
                anyhow::ensure!(self.target_id == "local", "local target ID must be `local`")
            }
            TargetLocation::Remote { worker_id } => {
                validate_identifier(&self.target_id)?;
                validate_identifier(worker_id)?;
                anyhow::ensure!(
                    &self.target_id == worker_id,
                    "remote target ID must match its worker ID"
                );
            }
        }
        Ok(())
    }
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq, Hash)]
#[serde(deny_unknown_fields)]
pub struct WorkspaceRef {
    pub project_id: String,
    pub task_id: String,
    pub source_sha: String,
}

impl WorkspaceRef {
    pub fn validate(&self) -> anyhow::Result<()> {
        validate_uuid(&self.project_id, "project ID")?;
        validate_uuid(&self.task_id, "task ID")?;
        anyhow::ensure!(is_valid_sha(&self.source_sha), "source SHA is invalid");
        Ok(())
    }
}

pub fn validate_identifier(value: &str) -> anyhow::Result<()> {
    validate_uuid(value, "identifier")
}

fn validate_uuid(value: &str, name: &str) -> anyhow::Result<()> {
    let id = uuid::Uuid::parse_str(value).map_err(|_| anyhow::anyhow!("{name} is invalid"))?;
    anyhow::ensure!(
        id.hyphenated().to_string() == value,
        "{name} must be a canonical UUID"
    );
    Ok(())
}

pub fn is_valid_sha(value: &str) -> bool {
    matches!(value.len(), 40 | 64) && value.bytes().all(|byte| byte.is_ascii_hexdigit())
}
