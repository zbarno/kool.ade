//! Detached sibling-instance spawn behind the Workspace menu's
//! 'Open workspace' action.
//!
//! Contract (CHG-003 "Switch Workspaces"):
//! - [`resolve_self_executable`] locates the running binary so the menu
//!   handler can respawn itself; on Linux this is the resolved
//!   `/proc/self/exe` path.
//! - [`spawn_sibling`] launches the given binary fire-and-forget: null
//!   stdio, WHOLESALE-INHERITED environment, and the `Child` is dropped
//!   immediately. The parent never holds, polls, or reaps the sibling; the
//!   kernel reparents it, so the sibling outlives the spawning window.
//!
//! A bare respawn is deterministically the initial (Welcome) screen:
//! `src/main.rs` never inspects argv and `PacketApp::default()` starts on
//! `Screen::Welcome`.

use std::path::{Path, PathBuf};

/// Locate the running executable to respawn as a sibling instance.
pub fn resolve_self_executable() -> Result<PathBuf, String> {
    std::env::current_exe()
        .map_err(|error| format!("could not locate the running Packet executable: {error}"))
}

/// Spawn a detached sibling Packet process.
///
/// Invariants:
/// - **Never `.env_clear()`.** The child inherits the parent's environment
///   wholesale, which is what carries `PACKET_HOME`,
///   `PACKET_TURN_TIMEOUT_SECS`, `PACKET_PI_BIN` (plus the Wayland/X
///   display variables) into the sibling. This is a deliberate, reviewed
///   code invariant; the `#[cfg(test)]` probe below verifies it at runtime.
/// - **Null stdio on all three streams.** Mandatory: the `Child` is dropped
///   at once, so any piped fd left undrained would fill its buffer and
///   stall the sibling mid-boot.
/// - **Fire-and-forget.** The `Child` is dropped without
///   `wait`/`wait_for`/pid tracking; a sibling exiting while this process
///   lives costs only a negligible zombie slot until the next reap cycle.
pub fn spawn_sibling(bin: &Path) -> Result<(), String> {
    std::process::Command::new(bin)
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        // Deliberately NO `.env_clear()` here — see the invariant above.
        .spawn()
        .map(|_child| ()) // dropped immediately: detach, never poll or reap
        .map_err(|error| {
            format!(
                "failed to launch the second Packet window at {}: {error}",
                bin.display()
            )
        })
}

/// Spawn a sibling window prefilled for the selected registered checkout.
pub fn spawn_sibling_in(bin: &Path, workspace: &Path) -> Result<(), String> {
    std::process::Command::new(bin)
        .current_dir(workspace)
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .spawn()
        .map(|_child| ())
        .map_err(|error| {
            format!(
                "failed to open a Packet window for {}: {error}",
                workspace.display()
            )
        })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::{Duration, Instant};

    /// Recorded pick (unit test plan 1): a trivial no-op utility present on
    /// the Linux x86_64 target hosts (D-11). `std::env::current_exe()` is
    /// deliberately WRONG here — it would recursively relaunch the test
    /// binary itself.
    fn harmless_binary() -> &'static str {
        const CANDIDATES: [&str; 2] = ["/usr/bin/true", "/bin/true"];
        CANDIDATES
            .iter()
            .copied()
            .find(|cand| Path::new(cand).is_file())
            .unwrap_or_else(|| panic!("expected /usr/bin/true or /bin/true on this Linux host"))
    }

    #[test]
    fn spawn_sibling_starts_an_innocuous_process_and_returns_without_blocking() {
        let began = Instant::now();
        let result = spawn_sibling(Path::new(harmless_binary()));
        assert!(result.is_ok(), "innocuous spawn failed: {result:?}");
        assert!(
            began.elapsed() < Duration::from_secs(2),
            "the parent must return immediately, not wait on the child"
        );
        // By contract there is no handle retained: the calling thread can
        // never observe the child's exit status afterward.
    }

    #[test]
    fn spawn_sibling_in_uses_the_selected_workspace_as_child_directory() {
        let dir = std::env::temp_dir().join(format!(
            "packet-spawn-workspace-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir_all(&dir).unwrap();
        let script = dir.join("probe.sh");
        let output = dir.join("cwd.txt");
        use std::os::unix::fs::PermissionsExt;
        std::fs::write(&script, format!("#!/bin/sh\npwd > {}\n", output.display())).unwrap();
        std::fs::set_permissions(&script, std::fs::Permissions::from_mode(0o755)).unwrap();
        spawn_sibling_in(&script, &dir).unwrap();
        let deadline = Instant::now() + Duration::from_secs(2);
        while Instant::now() < deadline && !output.exists() {
            std::thread::sleep(Duration::from_millis(20));
        }
        let observed = std::fs::read_to_string(&output).unwrap();
        assert_eq!(PathBuf::from(observed.trim()), dir.canonicalize().unwrap());
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn spawn_sibling_missing_binary_reports_failure_with_the_path_embedded() {
        let missing = Path::new("/nonexistent/packet-sibling-bin");
        let began = Instant::now();
        let error = spawn_sibling(missing).expect_err("a missing binary must not spawn");
        assert!(
            error.contains("/nonexistent/packet-sibling-bin"),
            "failing path must ride along in the operator-facing message: {error}"
        );
        assert!(
            began.elapsed() < Duration::from_secs(2),
            "a spawn failure must not block"
        );
    }

    #[test]
    fn resolve_self_executable_points_at_an_existing_executable_file() {
        let exe = resolve_self_executable().expect("the test binary must resolve");
        assert!(exe.exists(), "resolved path does not exist: {exe:?}");
        use std::os::unix::fs::PermissionsExt;
        let meta = std::fs::metadata(&exe).expect("metadata of the resolved executable");
        let mode = meta.permissions().mode();
        assert!(
            mode & 0o111 != 0,
            "the resolved binary must carry executable bits (got {mode:o})"
        );
    }

    /// Runtime mirror of the code-review invariant: the spawned sibling must
    /// observe variables the parent process already holds. The probe script
    /// records its `$HOME` to a unique file; a spawn that cleared the
    /// environment (`.env_clear()`) would yield an empty value instead.
    #[test]
    fn spawn_sibling_inherits_the_parent_environment_wholesale() {
        let home = std::env::var("HOME").expect("test host provides HOME");
        let stamp = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let dir =
            std::env::temp_dir().join(format!("packet-spawn-env-{}-{stamp}", std::process::id()));
        let script = dir.join("probe.sh");
        let out = dir.join("home.txt");
        std::fs::create_dir_all(&dir).expect("probe dir");
        use std::os::unix::fs::PermissionsExt;
        std::fs::write(
            &script,
            format!("#!/bin/sh\nprintf %s \"$HOME\" > {}\n", out.display()),
        )
        .expect("probe script");
        std::fs::set_permissions(&script, std::fs::Permissions::from_mode(0o755)).expect("chmod");

        spawn_sibling(&script).expect("probe script must spawn");

        let deadline = Instant::now() + Duration::from_secs(2);
        let mut observed = None;
        while Instant::now() < deadline {
            if let Ok(value) = std::fs::read_to_string(&out) {
                observed = Some(value);
                break;
            }
            std::thread::sleep(Duration::from_millis(20));
        }
        let _ = std::fs::remove_dir_all(&dir);
        assert_eq!(
            observed,
            Some(home),
            "the sibling must inherit $HOME exactly (environment travels wholesale)"
        );
    }
}
