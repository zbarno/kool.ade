//! OpenAI Codex CLI adapter. Codex's flags, JSONL event parsing, and readiness
//! checks stay behind the shared [`AiHarness`] interface.

use std::path::{Path, PathBuf};

use crate::error::AppError;

pub const CODEX_BINARY_ENV: &str = "KOOLADE_CODEX_BIN";
pub const CODEX_MODEL_ENV: &str = "KOOLADE_CODEX_MODEL";
const STDERR_LIMIT: usize = 100;

mod execute;
#[cfg(test)]
pub(super) use execute::{CodexEvent, command, normalize_effort, parse_event};
mod probe;

#[derive(Default)]
pub struct CodexHarness;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CodexProbeReport {
    pub status: String,
    pub version: Option<String>,
    pub diagnostic: String,
    pub binary: Option<PathBuf>,
    pub ready: bool,
    pub readiness: CodexReadiness,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CodexReadiness {
    Missing,
    Unusable,
    AuthenticationRequired,
    AuthenticationUnverified,
    Ready,
}

impl CodexHarness {
    pub fn locate_binary() -> Result<PathBuf, AppError> {
        if let Some(path) = crate::harness::manual_executable_path("codex")? {
            return Ok(path);
        }
        if let Ok(value) = std::env::var(CODEX_BINARY_ENV)
            && !value.trim().is_empty()
        {
            let path = PathBuf::from(&value);
            if executable(&path) {
                return Ok(path);
            }
            return Err(AppError::HarnessNotFound {
                detail: format!("{CODEX_BINARY_ENV} does not name an executable file"),
            });
        }
        for candidate in crate::harness::discovery::host_executable_candidates("codex") {
            if executable(&candidate) {
                return Ok(candidate);
            }
        }
        Err(AppError::HarnessNotFound {
            detail: format!(
                "codex executable not found in PATH or common install locations; set {CODEX_BINARY_ENV} to override"
            ),
        })
    }
}

fn executable(path: &Path) -> bool {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        path.metadata()
            .is_ok_and(|metadata| metadata.is_file() && metadata.permissions().mode() & 0o111 != 0)
    }
    #[cfg(not(unix))]
    {
        path.is_file()
    }
}

#[cfg(test)]
#[path = "codex_harness/tests.rs"]
mod tests;
