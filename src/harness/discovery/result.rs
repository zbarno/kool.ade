use serde::{Deserialize, Serialize};

use crate::execution::contract::ReadinessState;

const MAX_PROVIDER_BYTES: usize = 64;
const MAX_STATUS_BYTES: usize = 512;
const MAX_VERSION_BYTES: usize = 128;
const MAX_BINARY_PATH_BYTES: usize = 4096;
const MAX_DIAGNOSTIC_BYTES: usize = 2048;
const MAX_MODELS: usize = 500;
const MAX_MODEL_BYTES: usize = 256;

#[cfg(test)]
mod tests;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum ProbeKind {
    Executable,
    Version,
    Authentication,
    Models,
    Capability,
}

impl ProbeKind {
    pub fn key(self) -> &'static str {
        match self {
            Self::Executable => "executable",
            Self::Version => "version",
            Self::Authentication => "authentication",
            Self::Models => "models",
            Self::Capability => "capability",
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Self::Executable => "Executable",
            Self::Version => "Version",
            Self::Authentication => "Authentication",
            Self::Models => "Model catalog",
            Self::Capability => "Capability",
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum ProbeDiagnosticCode {
    ExecutableMissing,
    ExecutableInvalid,
    VersionUnavailable,
    AuthenticationRequired,
    ModelCatalogUnavailable,
    RuntimeUnavailable,
    ProviderProbeFailed,
    InvalidProviderResult,
}

impl ProbeDiagnosticCode {
    pub fn label(self) -> &'static str {
        match self {
            Self::ExecutableMissing => "executable_missing",
            Self::ExecutableInvalid => "executable_invalid",
            Self::VersionUnavailable => "version_unavailable",
            Self::AuthenticationRequired => "authentication_required",
            Self::ModelCatalogUnavailable => "model_catalog_unavailable",
            Self::RuntimeUnavailable => "runtime_unavailable",
            Self::ProviderProbeFailed => "provider_probe_failed",
            Self::InvalidProviderResult => "invalid_provider_result",
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum ProbeStatus {
    Available,
    Installed,
    NeedsAuthentication,
    Missing,
    Failed,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum ModelCatalogStatus {
    Fresh,
    Stale,
    Unavailable,
    NotChecked,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct ExecutionCapability {
    pub safe_for_planning: ReadinessState,
    pub safe_for_implementation: ReadinessState,
    pub runtime_ready: ReadinessState,
}

impl Default for ExecutionCapability {
    fn default() -> Self {
        Self {
            safe_for_planning: ReadinessState::Unknown,
            safe_for_implementation: ReadinessState::Unknown,
            runtime_ready: ReadinessState::Unknown,
        }
    }
}

/// Bounded, serializable provider-local evidence emitted by one discovery run.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct ProbeResult {
    pub schema_version: u8,
    pub provider_id: String,
    pub run_generation: u64,
    pub started_at_unix_ms: u64,
    pub duration_ms: u64,
    pub binary_path: Option<String>,
    pub version: Option<String>,
    pub authentication: ReadinessState,
    pub model_catalog_status: ModelCatalogStatus,
    pub models: Vec<String>,
    pub default_model: Option<String>,
    pub execution_capability: ExecutionCapability,
    pub status: ProbeStatus,
    pub display_status: String,
    pub failure_stage: Option<ProbeKind>,
    pub diagnostic_code: Option<ProbeDiagnosticCode>,
    pub diagnostic_message: Option<String>,
}

impl Default for ProbeResult {
    fn default() -> Self {
        Self {
            schema_version: 1,
            provider_id: String::new(),
            run_generation: 0,
            started_at_unix_ms: 0,
            duration_ms: 0,
            binary_path: None,
            version: None,
            authentication: ReadinessState::Unknown,
            model_catalog_status: ModelCatalogStatus::NotChecked,
            models: Vec::new(),
            default_model: None,
            execution_capability: ExecutionCapability::default(),
            status: ProbeStatus::Failed,
            display_status: String::new(),
            failure_stage: None,
            diagnostic_code: None,
            diagnostic_message: None,
        }
    }
}

impl ProbeResult {
    pub fn bounded(mut self) -> Self {
        if self.provider_id.is_empty()
            || self.provider_id.len() > MAX_PROVIDER_BYTES
            || has_control(&self.provider_id)
        {
            self.provider_id = "unknown".into();
            self.fail(
                ProbeKind::Capability,
                ProbeDiagnosticCode::InvalidProviderResult,
            );
        }
        let version_is_valid = bound_optional(&mut self.version, MAX_VERSION_BYTES);
        let binary_is_valid = bound_optional(&mut self.binary_path, MAX_BINARY_PATH_BYTES);
        if !version_is_valid || !binary_is_valid {
            self.fail(
                ProbeKind::Capability,
                ProbeDiagnosticCode::InvalidProviderResult,
            );
        }
        if self.display_status.trim().is_empty() || has_control(&self.display_status) {
            self.display_status = "Provider returned an invalid status".into();
            self.fail(
                ProbeKind::Capability,
                ProbeDiagnosticCode::InvalidProviderResult,
            );
        }
        bound_string(&mut self.display_status, MAX_STATUS_BYTES);
        if self.models.len() > MAX_MODELS
            || self.models.iter().any(|model| {
                model.trim().is_empty() || model.len() > MAX_MODEL_BYTES || has_control(model)
            })
            || self.default_model.as_ref().is_some_and(|model| {
                model.trim().is_empty() || model.len() > MAX_MODEL_BYTES || has_control(model)
            })
        {
            self.models.clear();
            self.default_model = None;
            self.model_catalog_status = ModelCatalogStatus::Unavailable;
            if matches!(self.status, ProbeStatus::Available | ProbeStatus::Installed) {
                self.failure_stage = Some(ProbeKind::Models);
                self.diagnostic_code = Some(ProbeDiagnosticCode::InvalidProviderResult);
                self.diagnostic_message
                    .get_or_insert_with(|| "Provider returned an invalid model catalog".into());
            }
        }
        if let Some(message) = self.diagnostic_message.as_mut() {
            if has_control(message) {
                *message = "Provider returned an invalid diagnostic".into();
                self.failure_stage.get_or_insert(ProbeKind::Capability);
                self.diagnostic_code
                    .get_or_insert(ProbeDiagnosticCode::InvalidProviderResult);
            }
            bound_string(message, MAX_DIAGNOSTIC_BYTES);
        }
        self
    }

    pub fn set_runtime_readiness(&mut self, runtime_ready: bool) {
        use ReadinessState::{Ready, Unavailable};

        self.execution_capability.runtime_ready = if runtime_ready { Ready } else { Unavailable };
        self.execution_capability.safe_for_implementation =
            match (self.execution_capability.safe_for_planning, runtime_ready) {
                (Ready, true) => Ready,
                (Ready, false) | (Unavailable, _) => Unavailable,
                _ => ReadinessState::Unknown,
            };
        if !runtime_ready
            && self.execution_capability.safe_for_planning == Ready
            && self.failure_stage.is_none()
            && self.diagnostic_code.is_none()
            && self.diagnostic_message.is_none()
        {
            self.failure_stage = Some(ProbeKind::Capability);
            self.diagnostic_code = Some(ProbeDiagnosticCode::RuntimeUnavailable);
        } else if runtime_ready
            && self.diagnostic_code == Some(ProbeDiagnosticCode::RuntimeUnavailable)
        {
            self.failure_stage = None;
            self.diagnostic_code = None;
        }
    }

    fn fail(&mut self, stage: ProbeKind, code: ProbeDiagnosticCode) {
        self.status = ProbeStatus::Failed;
        self.failure_stage = Some(stage);
        self.diagnostic_code = Some(code);
        self.execution_capability.safe_for_planning = ReadinessState::Unavailable;
        self.execution_capability.safe_for_implementation = ReadinessState::Unavailable;
    }
}

fn bound_optional(value: &mut Option<String>, max_bytes: usize) -> bool {
    let Some(current) = value.as_ref() else {
        return true;
    };
    if has_control(current) || current.len() > max_bytes {
        *value = None;
        return false;
    }
    true
}

fn bound_string(value: &mut String, max_bytes: usize) {
    if value.len() <= max_bytes {
        return;
    }
    let mut boundary = max_bytes;
    while !value.is_char_boundary(boundary) {
        boundary -= 1;
    }
    value.truncate(boundary);
}

fn has_control(value: &str) -> bool {
    value.chars().any(char::is_control)
}
