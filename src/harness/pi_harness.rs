//! Pi CLI implementation of [`super::AiHarness`] (SPECIFICATION.md §14–§19).
//!
//! Invocation shape (hermetic on purpose — no project settings bleed in):
//!
//! ```sh
//! pi -p --mode json --no-session --no-approve --no-context-files \
//!    --no-extensions --no-skills --no-prompt-templates \
//!    --append-system-prompt "<PLANNER PERSONA>" < prompt.txt
//! ```
//!
//! Working directory = the connected repository, so pi's own read/bash tools
//! inspect the codebase directly (§18).

use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use crate::error::AppError;
use crate::harness::pi_proc::StreamEvt;

mod read_budget;

#[cfg(test)]
use super::{AiHarness, PlanningRequest};

/// Name of the environment override for locating the pi executable.
pub const PI_BINARY_ENV: &str = "KOOLADE_PI_BIN";
/// Wall-clock budget for the availability probe.
const CHECK_TIMEOUT: Duration = Duration::from_secs(10);
/// Silence tolerance for a running pi process: when neither stdout nor
/// stderr yields a line for this long, the run is presumed hung (deadlocked
/// tool, wedged transport) and is killed. The 12 h `TURN_TIMEOUT` ceiling
/// stays the hard budget; this converts a hung run from hours into minutes.
pub const STALL_TIMEOUT: Duration = Duration::from_secs(10 * 60);
/// Environment override (seconds) for the stall tolerance.
pub const STALL_TIMEOUT_ENV: &str = "KOOLADE_HARNESS_STALL_SECS";

pub fn configured_stall_timeout() -> Duration {
    stall_timeout_from_raw(std::env::var(STALL_TIMEOUT_ENV).ok().as_deref())
}

/// Overrides above this (1 year) are treated as typos, not policies.
const STALL_TIMEOUT_CAP: u64 = 365 * 24 * 3600;

fn stall_timeout_from_raw(raw: Option<&str>) -> Duration {
    raw.and_then(|value| value.trim().parse::<u64>().ok())
        .filter(|seconds| (1..=STALL_TIMEOUT_CAP).contains(seconds))
        .map(Duration::from_secs)
        .unwrap_or(STALL_TIMEOUT)
}
/// Home-relative install sites scanned LAST by [`PiHarness::locate_binary`]
/// (`$HOME/<site>/pi`, in this order, after `KOOLADE_PI_BIN` and `PATH`).
/// SINGLE SOURCE for that tier: the F-16 in-app setup guide composes its
/// discovery-order lines from this constant, so the rendered order can never
/// drift from the executed order (D-15).
pub const COMMON_HOME_SITES: [&str; 3] = [".npm-global/bin", ".local/bin", ".pi/bin"];

/// The single MVP harness. Construct once; the app may hold it behind an Arc.
#[derive(Default)]
pub struct PiHarness;

/// Display-purpose snapshot from [`PiHarness::probe_report`], consumed by the
/// F-16 setup-guide section of the settings card (D-15). Purely rendered —
/// nothing durable is derived from it, and it never gates turns/connect/save.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProbeReport {
    /// Status line in `label()` shape: `"pi {version}"` on success, or
    /// `"pi (unavailable: {headline})"` otherwise. Byte-identical to
    /// `PiHarness::label()` observed in the same environment state
    /// (parity invariant, pinned by test).
    pub status: String,
    /// Actionable diagnosis (`AppError::detail`): names the bad override or
    /// the searched sources. Empty when `ok`.
    pub diagnostic: String,
    /// The winning binary, when discovery itself succeeded (present even if
    /// the version probe later failed, so the guide shows WHICH binary was
    /// found beside why it failed). `None` when discovery failed outright.
    pub binary: Option<PathBuf>,
    /// `true` only when discovery, version and required-capability probes succeed.
    pub ok: bool,
    /// The executable/version work, but provider credentials or model config
    /// still need operator attention.
    pub configuration_required: bool,
}

impl PiHarness {
    /// Locate the pi executable: `KOOLADE_PI_BIN` → `PATH` → common homes.
    pub fn locate_binary() -> Result<std::path::PathBuf, AppError> {
        if let Ok(explicit) = std::env::var(PI_BINARY_ENV)
            && !explicit.is_empty()
        {
            let p = std::path::PathBuf::from(&explicit);
            if p.is_file() && is_executable(&p) {
                return Ok(p);
            }
            return Err(AppError::HarnessNotFound {
                detail: format!("{PI_BINARY_ENV}={explicit} is not an executable file"),
            });
        }
        if let Some(path_var) = std::env::var_os("PATH") {
            for dir in std::env::split_paths(&path_var) {
                let cand = dir.join("pi");
                if cand.is_file() && is_executable(&cand) {
                    return Ok(cand);
                }
            }
        }
        let home = std::env::var_os("HOME").map(|h| h.to_string_lossy().into_owned());
        if let Some(home) = home {
            for site in COMMON_HOME_SITES {
                let cand = format!("{home}/{site}/pi");
                let p = Path::new(&cand);
                if p.is_file() && is_executable(p) {
                    return Ok(p.to_path_buf());
                }
            }
        }
        Err(AppError::HarnessNotFound {
            detail: format!(
                "pi executable not found (searched PATH and common install locations; \
                 set {PI_BINARY_ENV} to override)"
            ),
        })
    }

    /// Display-purpose probe for the F-16 setup guide (D-15): locates the
    /// binary and, when found, runs the version probe. Panic-free — every
    /// arm is matched and spawn/process errors map to `AppError`.
    ///
    /// Construction table (`status` keeps byte parity with
    /// [`label`](super::AiHarness::label)):
    /// * locate fails           → `ok=false, binary=None,       status=pi (unavailable: {headline}), diagnostic=detail`
    /// * locate ok, probe fails → `ok=false, binary=Some(exe),  status=pi (unavailable: {headline}), diagnostic=detail`
    /// * locate ok, probe ok    → `ok=true,  binary=Some(exe),  status=pi {version},                   diagnostic=""`
    pub fn probe_report() -> ProbeReport {
        match Self::locate_binary() {
            Err(e) => ProbeReport {
                status: format!("pi (unavailable: {})", e.headline()),
                diagnostic: e.detail(),
                binary: None,
                ok: false,
                configuration_required: false,
            },
            Ok(exe) => match Self::check_binary(&exe) {
                Ok(version) => match crate::harness::pi_sandbox::provider_configuration_error() {
                    Some(diagnostic) => ProbeReport {
                        status: format!("pi {version}"),
                        diagnostic,
                        binary: Some(exe),
                        ok: false,
                        configuration_required: true,
                    },
                    None => ProbeReport {
                        status: format!("pi {version}"),
                        diagnostic: String::new(),
                        binary: Some(exe),
                        ok: true,
                        configuration_required: false,
                    },
                },
                Err(e) => ProbeReport {
                    status: format!("pi (unavailable: {})", e.headline()),
                    diagnostic: e.detail(),
                    binary: Some(exe),
                    ok: false,
                    configuration_required: false,
                },
            },
        }
    }

    /// Run `<exe> --version` and pull the display-only version out of the
    /// first stdout line: 10 s poll budget, 2 s settle. Body lifted verbatim
    /// from `check_available`, which now delegates here (behavior and the
    /// `AiHarness` contract are unchanged).
    fn probe_version(exe: &Path) -> Result<String, AppError> {
        let task = crate::harness::pi_proc::spawn(
            &[exe.to_string_lossy().into_owned(), "--version".into()],
            Path::new("."),
        )?;
        let deadline = Instant::now() + CHECK_TIMEOUT;
        let mut version = String::new();
        let mut exited = None;
        while Instant::now() < deadline {
            match task.next_line(Duration::from_millis(100)) {
                Some(StreamEvt::Stdout(line)) => {
                    if version.is_empty() {
                        version = line.trim().to_string();
                    }
                }
                Some(StreamEvt::Exited(ok)) => {
                    exited = Some(ok);
                    break;
                }
                _ => {}
            }
        }
        let exit = exited.or_else(|| task.settle(Duration::from_secs(2)));
        if exit != Some(true) || version.is_empty() {
            return Err(AppError::HarnessFailed {
                reason: "pi --version failed".into(),
                stderr_tail: String::new(),
            });
        }
        Ok(extract_version(&version))
    }

    fn check_binary(exe: &Path) -> Result<String, AppError> {
        let version = Self::probe_version(exe)?;
        capabilities::validate_all(exe)?;
        Ok(version)
    }
}

fn is_executable(p: &Path) -> bool {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        p.metadata()
            .map(|m| m.permissions().mode() & 0o111 != 0)
            .unwrap_or(false)
    }
    #[cfg(not(unix))]
    {
        let _ = p;
        true
    }
}

fn compress_completed_events(path: &Path) -> std::io::Result<()> {
    let archived = path.with_extension("jsonl.gz");
    let temp = path.with_extension("jsonl.gz.tmp");
    let output = std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&temp)?;
    let result = std::process::Command::new("gzip")
        .arg("-c")
        .arg(path)
        .stdout(std::process::Stdio::from(output))
        .stderr(std::process::Stdio::null())
        .status();
    match result {
        Ok(status) if status.success() => {
            if let Err(error) = std::fs::rename(&temp, &archived) {
                let _ = std::fs::remove_file(&temp);
                return Err(error);
            }
            std::fs::remove_file(path)
        }
        Ok(status) => {
            let _ = std::fs::remove_file(&temp);
            Err(std::io::Error::other(format!("gzip exited with {status}")))
        }
        Err(error) => {
            let _ = std::fs::remove_file(&temp);
            Err(error)
        }
    }
}

fn tail(lines: &[String]) -> String {
    let joined = lines.join("\n");
    const CAP: usize = 16_000;
    if joined.chars().count() <= CAP {
        return joined;
    }
    let mut chars: Vec<char> = joined.chars().rev().take(CAP).collect();
    chars.reverse();
    let chars: String = chars.into_iter().collect();
    format!("…{chars}")
}

/// Pull a version string out of `pi --version` output ("pi - v0.84.4").
fn extract_version(out: &str) -> String {
    let token = out
        .split_whitespace()
        .find(|t| {
            t.trim_start_matches(['v', '-'])
                .starts_with(|c: char| c.is_ascii_digit())
        })
        .map(str::to_string)
        .unwrap_or_else(|| {
            out.split_whitespace()
                .next()
                .unwrap_or_default()
                .to_string()
        });
    token.trim_start_matches('v').to_string()
}

mod capabilities;
mod diagnostics;
mod execute;

#[cfg(test)]
mod tests;
