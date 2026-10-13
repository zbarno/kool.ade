use serde::{Deserialize, Serialize};

/// Controller-side routing choice. It never grants remote authority by itself;
/// `Auto` still requires an enabled, enrolled worker matching the task scope.
#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(tag = "kind", content = "worker_id", rename_all = "snake_case")]
pub enum TargetPreference {
    Local,
    Worker(String),
    Auto,
}
