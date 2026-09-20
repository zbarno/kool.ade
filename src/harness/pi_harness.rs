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
use crate::harness::pi_events::EventFold;
use crate::harness::pi_extract::extract_json_object;
use crate::harness::pi_proc::StreamEvt;

use super::{AiHarness, HarnessOutcome, PlanningRequest, TurnEnvelope};

/// Name of the environment override for locating the pi executable.
pub const PI_BINARY_ENV: &str = "PACKET_PI_BIN";
/// Wall-clock budget for the availability probe.
const CHECK_TIMEOUT: Duration = Duration::from_secs(10);
/// Silence tolerance for a running pi process: when neither stdout nor
/// stderr yields a line for this long, the run is presumed hung (deadlocked
/// tool, wedged transport) and is killed. The 12 h `TURN_TIMEOUT` ceiling
/// stays the hard budget; this converts a hung run from hours into minutes.
pub const STALL_TIMEOUT: Duration = Duration::from_secs(10 * 60);
/// Environment override (seconds) for the stall tolerance.
pub const STALL_TIMEOUT_ENV: &str = "PACKET_HARNESS_STALL_SECS";

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
/// (`$HOME/<site>/pi`, in this order, after `PACKET_PI_BIN` and `PATH`).
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
    /// `true` only when discovery AND the version probe both succeeded.
    pub ok: bool,
}

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
            },
            Ok(exe) => match Self::probe_version(&exe) {
                Ok(version) => ProbeReport {
                    status: format!("pi {version}"),
                    diagnostic: String::new(),
                    binary: Some(exe),
                    ok: true,
                },
                Err(e) => ProbeReport {
                    status: format!("pi (unavailable: {})", e.headline()),
                    diagnostic: e.detail(),
                    binary: Some(exe),
                    ok: false,
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
        Self::probe_version(&exe)
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
            "--thinking".into(),
            req.reasoning_level.clone(),
        ]);

        if req.read_only {
            argv.push("--no-tools".into());
        }
        if req.implementation {
            argv.retain(|arg| arg != "--no-context-files");
        }
        let mut diagnostics = if req.implementation {
            let output = std::process::Command::new("git")
                .args(["rev-parse", "--path-format=absolute", "--git-common-dir"])
                .current_dir(&req.repo_root)
                .output()
                .map_err(|e| AppError::Other(e.to_string()))?;
            if !output.status.success() {
                return Err(AppError::Other(
                    "Cannot locate harness diagnostics directory".into(),
                ));
            }
            let directory = PathBuf::from(String::from_utf8_lossy(&output.stdout).trim())
                .join("packet-harness");
            std::fs::create_dir_all(&directory).map_err(|e| AppError::Other(e.to_string()))?;
            let path = directory.join(format!(
                "{}-events.jsonl",
                chrono::Utc::now().timestamp_nanos_opt().unwrap_or_default()
            ));
            let file = std::fs::OpenOptions::new()
                .create_new(true)
                .write(true)
                .open(&path)
                .map_err(|e| AppError::Other(e.to_string()))?;
            Some((path, file))
        } else {
            None
        };
        let task = crate::harness::pi_proc::spawn_with_input(
            &argv,
            &req.repo_root,
            Some(req.prompt_body.clone()),
        )?;
        let deadline = Instant::now() + req.timeout;
        let stall_limit = configured_stall_timeout();
        let stall_secs = stall_limit.as_secs();
        let mut last_output = Instant::now();
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
                Err(crate::harness::pi_proc::PollState::Pending) => {
                    if last_output.elapsed() >= stall_limit {
                        task.kill();
                        let _ = task.settle(Duration::from_secs(3));
                        return Err(AppError::HarnessFailed {
                            reason: format!(
                                "harness stalled: pi produced no output for {stall_secs}s and was presumed hung"
                            ),
                            stderr_tail: tail(&stderr_tail),
                        });
                    }
                    continue;
                }
                Ok(StreamEvt::Stdout(line)) => {
                    last_output = Instant::now();
                    if let Some((_, file)) = diagnostics.as_mut() {
                        use std::io::Write;
                        writeln!(file, "{line}").map_err(|e| {
                            AppError::Other(format!("Cannot write harness diagnostics: {e}"))
                        })?;
                    }
                    crate::harness::pi_events::fold_line(&line, &mut fold);
                    preview_dirty = true;
                }
                Ok(StreamEvt::Stderr(line)) => {
                    last_output = Instant::now();
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
                reason: format!(
                    "pi finished but produced no final assistant message ({} parsed events, {} unparsed lines, agent_end={}; diagnostics: {})",
                    fold.events_seen,
                    fold.unparsed_lines,
                    fold.saw_agent_end,
                    diagnostics
                        .as_ref()
                        .map(|(path, _)| path.display().to_string())
                        .unwrap_or_else(|| "not recorded for planning turns".into())
                ),
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

    /// A bogus-but-set override FAILS FAST through the report path: no
    /// binary, no fall-through claim, the exact coarse status line, and the
    /// actionable override text in `diagnostic`. Parity invariant: under the
    /// same forced state the report status is byte-identical to `label()`.
    #[test]
    fn probe_report_bogus_override_fast_fails_with_label_parity() {
        let _shield = crate::core::gitops::test_support::shield("probe-bogus-env");
        let prev = std::env::var_os(PI_BINARY_ENV);
        // SAFETY: serialized by the shield; the pre-existing value is
        // restored unconditionally before any assertion runs.
        unsafe { std::env::set_var(PI_BINARY_ENV, "/nonexistent-packet-selftest/pi") };
        let rep = PiHarness::probe_report();
        // Trait label under the IDENTICALLY forced state (captured before
        // restoring so the parity comparison spans one environment state).
        let label = (<PiHarness as AiHarness>::label)(&PiHarness);
        unsafe {
            match prev {
                Some(v) => std::env::set_var(PI_BINARY_ENV, v),
                None => std::env::remove_var(PI_BINARY_ENV),
            }
        };
        assert!(!rep.ok, "bogus override must not report ok");
        assert!(rep.binary.is_none(), "failed discovery has no binary");
        assert_eq!(rep.status, "pi (unavailable: Pi harness not found)");
        assert!(
            rep.diagnostic.contains("is not an executable file"),
            "diagnostic must carry the fast-fail text, got: {}",
            rep.diagnostic
        );
        // No fall-through claim: the searched-sources boilerplate is absent
        // because an invalid override stops discovery before PATH/home scans.
        assert!(
            !rep.diagnostic.contains("searched PATH"),
            "got: {}",
            rep.diagnostic
        );
        assert_eq!(rep.status, label, "status drifted from label()");
    }

    /// SIMULATED NOT-FOUND (host-agnostic): exactly the "no pi anywhere
    /// discoverable and no override set" state — every PATH entry that
    /// actually provides a `pi` is dropped, HOME points nowhere, and the
    /// override is unset — while the rest of the machine (git et al.) keeps
    /// working. Pins the exact unavailable values, including the actionable
    /// override hint in `diagnostic`. Green with or without pi installed.
    #[test]
    fn probe_report_without_any_discoverable_pi_names_the_override() {
        // Restore-on-drop guards so an assertion failure cannot leak the
        // narrowed environment into siblings.
        struct EnvRestore(&'static str, Option<std::ffi::OsString>);
        impl Drop for EnvRestore {
            fn drop(&mut self) {
                // SAFETY: serialized by the global test lock held below; the
                // ambient value is restored exactly once, unconditionally.
                unsafe {
                    match self.1.clone() {
                        Some(v) => std::env::set_var(self.0, v),
                        None => std::env::remove_var(self.0),
                    }
                }
            }
        }
        let _shield = crate::core::gitops::test_support::shield("probe-notfound-env");
        let prev_home = std::env::var_os("HOME");
        let prev_path = std::env::var_os("PATH");
        let prev_override = std::env::var_os(PI_BINARY_ENV);
        let keep: Vec<PathBuf> = match prev_path.as_ref() {
            Some(p) => std::env::split_paths(&p)
                .filter(|d| !d.join("pi").is_file())
                .collect(),
            None => Vec::new(),
        };
        let narrowed = std::env::join_paths(if keep.is_empty() {
            [PathBuf::from("/usr/bin"), PathBuf::from("/bin")]
                .into_iter()
                .collect::<Vec<_>>()
        } else {
            keep.clone()
        })
        .unwrap_or_else(|_| "/usr/bin:/bin".into());
        // SAFETY: env mutations are guarded by the global test lock, and
        // every one of them is restored by the Drop guards (even on panic).
        unsafe {
            std::env::set_var("HOME", "/nonexistent-packet-selftest-home");
            std::env::set_var("PATH", narrowed);
            std::env::remove_var(PI_BINARY_ENV);
        }
        let _guard_home = EnvRestore("HOME", prev_home);
        let _guard_path = EnvRestore("PATH", prev_path);
        let _guard_override = EnvRestore(PI_BINARY_ENV, prev_override);
        let rep = PiHarness::probe_report();
        assert!(!rep.ok, "undiscoverable pi must not report ok");
        assert!(rep.binary.is_none(), "no binary may win: {:?}", rep.binary);
        assert_eq!(rep.status, "pi (unavailable: Pi harness not found)");
        assert!(
            rep.diagnostic.contains("set PACKET_PI_BIN to override"),
            "diagnostic must name the override remedy: {}",
            rep.diagnostic
        );
    }

    /// Host-agnostic invariant (green on pi-installed and pi-less CI alike):
    /// status always keeps the label shape, `ok` agrees with the binary and
    /// the "(unavailable" marker, and a reported binary is a real file.
    #[test]
    fn probe_report_invariants_hold_on_any_host() {
        let rep = PiHarness::probe_report();
        assert!(rep.status.starts_with("pi "), "status: {}", rep.status);
        if rep.ok {
            assert!(rep.binary.is_some(), "ok requires a winning binary");
            assert!(
                !rep.status.contains("(unavailable"),
                "status: {}",
                rep.status
            );
            assert!(rep.diagnostic.is_empty(), "diagnostic: {}", rep.diagnostic);
        } else {
            assert!(
                rep.status.contains("(unavailable"),
                "status: {}",
                rep.status
            );
        }
        if let Some(bin) = &rep.binary {
            assert!(bin.is_file(), "reported binary vanished: {bin:?}");
        }
    }

    #[test]
    fn stall_timeout_defaults_and_parses_override() {
        assert_eq!(stall_timeout_from_raw(None), STALL_TIMEOUT);
        assert_eq!(stall_timeout_from_raw(Some("")), STALL_TIMEOUT);
        assert_eq!(stall_timeout_from_raw(Some("garbage")), STALL_TIMEOUT);
        assert_eq!(stall_timeout_from_raw(Some("0")), STALL_TIMEOUT);
        assert_eq!(stall_timeout_from_raw(Some("-5")), STALL_TIMEOUT);
        assert_eq!(stall_timeout_from_raw(Some("90")), Duration::from_secs(90));
        assert_eq!(stall_timeout_from_raw(Some(" 120 ")), Duration::from_secs(120));
        // Absurd overrides clamp to the 1-year cap boundary -> default.
        assert_eq!(
            stall_timeout_from_raw(Some(&(STALL_TIMEOUT_CAP + 1).to_string())),
            STALL_TIMEOUT
        );
        assert_eq!(
            stall_timeout_from_raw(Some(&u64::MAX.to_string())),
            STALL_TIMEOUT
        );
        assert_eq!(
            stall_timeout_from_raw(Some(&STALL_TIMEOUT_CAP.to_string())),
            Duration::from_secs(STALL_TIMEOUT_CAP)
        );
    }
}
