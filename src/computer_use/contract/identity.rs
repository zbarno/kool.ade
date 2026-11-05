use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ComputerSessionClassV1 {
    Disposable,
    PersonalPersistent,
}

/// Stable controller-owned identity. Each identifier is a canonical UUID.
#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct ComputerIdentityV1 {
    pub project_id: String,
    pub task_id: String,
    pub user_id: String,
    pub session_id: String,
}

impl ComputerIdentityV1 {
    pub fn validate(&self) -> Result<(), &'static str> {
        for value in [
            &self.project_id,
            &self.task_id,
            &self.user_id,
            &self.session_id,
        ] {
            let parsed = uuid::Uuid::parse_str(value).map_err(|_| "identity must use UUIDs")?;
            if parsed.to_string() != *value {
                return Err("identity UUIDs must be canonical lowercase values");
            }
        }
        Ok(())
    }
}
