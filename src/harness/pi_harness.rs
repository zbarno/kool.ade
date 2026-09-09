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

use std::path::Path;
use std::time::{Duration, Instant};

use crate::error::AppError;
use crate::harness::pi_events::EventFold;
use crate::harness::pi_extract::extract_json_object;
use crate::harness::pi_proc::StreamEvt;

use super::{AiHarness, HarnessOutcome, PlanningRequest, TurnEnvelope};

/// Name of the environment override for locating the pi executable.
pub const PI_BINARY_ENV: &str = "PACKET_PI_BIN";
/// Wall-clock budget for the availability probe.
const CHECK_TIMEOUT: Duration = Duration::from_secs(10);

/// The single MVP harness. Construct once; the app may hold it behind an Arc.
#[derive(Default)]
pub struct PiHarness;

impl PiHarness {
    /// Locate the pi executable: `PACKET_PI_BIN` → `PATH` → common homes.
    pub fn locate_binary() -> Result<std::path::PathBuf, AppError> {
        if let Ok(explicit) = std::env::var(PI_BINARY_ENV) {
            if !explicit.is_empty() {
                let p = std::path::PathBuf::from(&explicit);
                if p.is_file() && is_executable(&p) {
                    return Ok(p);
                }
                return Err(AppError::HarnessNotFound {
                    detail: format!("{PI_BINARY_ENV}={explicit} is not an executable file"),
                });
            }
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
            for cand in [
                format!("{home}/.npm-global/bin/pi"),
                format!("{home}/.local/bin/pi"),
                format!("{home}/.pi/bin/pi"),
            ] {
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

impl AiHarness for PiHarness {
    fn label(&self) -> String {
        match self.check_available() {
            Ok(version) => format!("pi {version}"),
            Err(e) => format!("pi (unavailable: {})", e.headline()),
        }
    }

    fn check_available(&self) -> Result<String, AppError> {
        let exe = Self::locate_binary()?;
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

    fn execute(&self, req: &PlanningRequest) -> Result<HarnessOutcome, AppError> {
        let exe = Self::locate_binary()?;
        let mut argv = vec![exe.to_string_lossy().into_owned()];
        argv.extend([
            "-p".into(),
            "--mode".into(),
            "json".into(),
            "--no-session".into(),
            "--no-approve".into(),
            "--no-context-files".into(),
            "--no-extensions".into(),
            "--no-skills".into(),
            "--no-prompt-templates".into(),
            "--append-system-prompt".into(),
            req.system_instructions.clone(),
        ]);

        if req.implementation {
            argv.retain(|arg| arg != "--no-context-files");
        }
        let task = crate::harness::pi_proc::spawn_with_input(
            &argv,
            &req.repo_root,
            Some(req.prompt_body.clone()),
        )?;
        let deadline = Instant::now() + req.timeout;
        let mut fold = EventFold::default();
        let mut stderr_tail: Vec<String> = Vec::new();
        let mut last_preview = super::LiveProgress::default();
        let mut last_emit = Instant::now() - Duration::from_millis(50);
        let mut preview_dirty = false;

        loop {
            if req.cancel.load(std::sync::atomic::Ordering::Relaxed) {
                task.kill();
                let _ = task.settle(Duration::from_secs(3));
                return Err(AppError::HarnessFailed {
                    reason: "cancelled by user".into(),
                    stderr_tail: tail(&stderr_tail),
                });
            }
            if Instant::now() >= deadline {
                task.kill();
                let _ = task.settle(Duration::from_secs(3));
                return Err(AppError::HarnessTimedOut {
                    secs: req.timeout.as_secs(),
                });
            }
            if preview_dirty && last_emit.elapsed() >= Duration::from_millis(50) {
                let preview = fold.preview();
                if preview != last_preview {
                    let _ = req.progress_tx.send(preview.clone());
                    last_preview = preview;
                }
                preview_dirty = false;
                last_emit = Instant::now();
            }
            // NOTE: `Pending` is NOT an error state — the first event from a
            // cold harness can lag well past one poll window.
            match task.poll_next(Duration::from_millis(200)) {
                Err(crate::harness::pi_proc::PollState::Pending) => continue,
                Ok(StreamEvt::Stdout(line)) => {
                    crate::harness::pi_events::fold_line(&line, &mut fold);
                    preview_dirty = true;
                }
                Ok(StreamEvt::Stderr(line)) => {
                    stderr_tail.push(line);
                    if stderr_tail.len() > 100 {
                        stderr_tail.remove(0);
                    }
                }
                Ok(StreamEvt::Exited(ok)) => {
                    if !ok {
                        return Err(AppError::HarnessFailed {
                            reason: fold
                                .error_hint
                                .clone()
                                .unwrap_or_else(|| "pi exited with a failure code".into()),
                            stderr_tail: tail(&stderr_tail),
                        });
                    }
                    break;
                }
                Err(crate::harness::pi_proc::PollState::Closed) => {
                    // Pipe disconnected without an Exited event (defensive).
                    if !fold.saw_agent_end && fold.final_assistant_text.is_empty() {
                        return Err(AppError::HarnessFailed {
                            reason: "stream ended unexpectedly".into(),
                            stderr_tail: tail(&stderr_tail),
                        });
                    }
                    break;
                }
            }
        }

        let _ = req.progress_tx.send(fold.preview());
        let final_text = std::mem::take(&mut fold.final_assistant_text);
        if final_text.trim().is_empty() {
            return Err(AppError::HarnessFailed {
                reason: "pi finished but produced no final assistant message".into(),
                stderr_tail: tail(&stderr_tail),
            });
        }
        let envelope = extract_json_object(&final_text)
            .and_then(|obj| serde_json::from_str::<TurnEnvelope>(&obj).ok());
        Ok(HarnessOutcome {
            final_text,
            envelope,
            stderr_tail: tail(&stderr_tail),
        })
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn version_extraction_handles_shapes() {
        assert_eq!(extract_version("pi - v0.84.4"), "0.84.4");
        assert_eq!(extract_version("0.84.4"), "0.84.4");
        assert_eq!(extract_version("weird output"), "weird");
    }

    #[test]
    fn tail_caps_long_streams() {
        let lines: Vec<String> = (0..100)
            .map(|i| format!("line {i} {}", "x".repeat(500)))
            .collect();
        let t = tail(&lines);
        assert!(t.chars().count() <= 16_001);
        assert!(t.starts_with('…'));
    }

    #[test]
    fn explicit_env_path_is_honored_when_invalid() {
        let prev = std::env::var_os(PI_BINARY_ENV);
        // SAFETY: unit-test process-env mutation; the pre-existing value is
        // restored unconditionally before the assertion runs.
        unsafe { std::env::set_var(PI_BINARY_ENV, "/definitely/not/a/pi/binary") };
        let res = PiHarness::locate_binary();
        unsafe {
            match prev {
                Some(v) => std::env::set_var(PI_BINARY_ENV, v),
                None => std::env::remove_var(PI_BINARY_ENV),
            }
        };
        assert!(matches!(res, Err(AppError::HarnessNotFound { .. })));
    }
}
