use std::io::Cursor;

use super::*;

#[test]
fn retained_qemu_output_is_bounded_and_overflow_is_reported() {
    let output = vec![b'x'; MAX_RETAINED_OUTPUT + 17];
    let (retained, overflowed) = retain_bounded(Cursor::new(output)).unwrap();
    assert_eq!(retained.len(), MAX_RETAINED_OUTPUT);
    assert!(overflowed);
}

#[test]
fn retained_qemu_output_preserves_small_payloads() {
    let (retained, overflowed) = retain_bounded(Cursor::new(b"QEMU version\n")).unwrap();
    assert_eq!(retained, b"QEMU version\n");
    assert!(!overflowed);
}

#[cfg(unix)]
#[test]
fn absent_qemu_executable_is_a_typed_not_found_result() {
    let error = qemu_output_from(
        "/definitely/not/a/qemu/executable",
        &["--version"],
        COMMAND_TIMEOUT,
    )
    .unwrap_err();
    assert_eq!(error.kind(), std::io::ErrorKind::NotFound);
}

#[test]
fn cpuinfo_virtualization_flags_are_only_evidence_and_parse_failures_stay_unknown() {
    assert_eq!(
        cpu_virtualization_flags_from_info("processor : 0\nflags : fpu vmx sse2\n"),
        Some(true)
    );
    assert_eq!(
        cpu_virtualization_flags_from_info("processor : 0\nflags : fpu svm sse2\n"),
        Some(true)
    );
    assert_eq!(
        cpu_virtualization_flags_from_info("processor : 0\nflags : fpu sse2\n"),
        Some(false)
    );
    assert_eq!(cpu_virtualization_flags_from_info("processor : 0\n"), None);
}

#[test]
fn system_virtualization_detection_requires_consistent_status_and_output() {
    assert_eq!(
        virtualization_from_status_and_output(Some(0), b"kvm\n"),
        Some(true)
    );
    assert_eq!(
        virtualization_from_status_and_output(Some(1), b"none\n"),
        Some(false)
    );
    assert_eq!(
        virtualization_from_status_and_output(Some(0), b"none\n"),
        None
    );
    assert_eq!(virtualization_from_status_and_output(Some(1), b""), None);
    assert_eq!(
        virtualization_from_status_and_output(Some(2), b"none\n"),
        None
    );
    assert_eq!(virtualization_from_status_and_output(None, b"kvm\n"), None);
    assert_eq!(
        virtualization_from_status_and_output(Some(0), &[0xff]),
        None
    );
}

#[cfg(target_os = "linux")]
#[test]
// Production boundary: a bounded fixed-command probe cannot hang when its
// launcher keeps stdout open after the parent exits. Requires /bin/sh and
// sleep; no daemon, network, or persistent setup is needed.
fn timeout_kills_descendant_that_inherited_probe_stdout() {
    let started = std::time::Instant::now();
    let result = qemu_output_from(
        "/bin/sh",
        &["-c", "sleep 10 & wait"],
        std::time::Duration::from_millis(100),
    );
    assert_eq!(result.unwrap_err().kind(), std::io::ErrorKind::TimedOut);
    assert!(started.elapsed() < std::time::Duration::from_secs(2));
}

#[cfg(target_os = "linux")]
#[test]
// Production boundary: output draining remains within the deadline after the
// probe leader exits. Requires /bin/sh and sleep; setup cost is negligible.
fn timeout_also_covers_descendant_after_probe_leader_exits() {
    let started = std::time::Instant::now();
    let result = qemu_output_from(
        "/bin/sh",
        &["-c", "sleep 10 & exit 0"],
        std::time::Duration::from_millis(100),
    );
    assert_eq!(result.unwrap_err().kind(), std::io::ErrorKind::TimedOut);
    assert!(started.elapsed() < std::time::Duration::from_secs(2));
}

#[cfg(target_os = "linux")]
#[test]
// Production boundary: normal probe completion must clean up same-group
// helpers even when they close the captured output pipe. Requires /bin/sh and
// sleep; the marker lives in a unique temporary path and is removed afterward.
fn successful_probe_kills_helper_that_closed_its_output() {
    let marker = std::env::temp_dir().join(format!(
        "koolade-probe-helper-{}-{}.marker",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    let script = format!(
        "(sleep 0.3; touch '{}') >/dev/null 2>&1 & exit 0",
        marker.display()
    );
    let result = qemu_output_from("/bin/sh", &["-c", &script], COMMAND_TIMEOUT).unwrap();
    assert!(result.is_some());
    std::thread::sleep(std::time::Duration::from_millis(500));
    assert!(!marker.exists(), "detached probe helper was not terminated");
}

#[cfg(target_os = "linux")]
#[test]
// Production boundary: seccomp must prevent a hostile helper from escaping
// the process group with setsid. Requires /bin/sh, util-linux setsid, and
// sleep; marker and process setup are temporary and low cost.
fn probe_helper_cannot_escape_its_process_group_with_setsid() {
    let temp = tempfile::tempdir().unwrap();
    let marker = temp.path().join("escaped-helper");
    let script = format!(
        "/usr/bin/setsid /bin/sh -c 'sleep 0.3; touch \"$1\"' child '{}' >/dev/null 2>&1 & exit 0",
        marker.display()
    );
    assert!(
        qemu_output_from("/bin/sh", &["-c", &script], COMMAND_TIMEOUT)
            .unwrap()
            .is_some()
    );
    std::thread::sleep(std::time::Duration::from_millis(500));
    assert!(!marker.exists(), "setsid helper escaped probe confinement");
}

#[cfg(target_os = "linux")]
#[test]
// Production boundary: every host probe runs with no_new_privs before exec so
// file capabilities or setuid metadata cannot grant privilege. Reads the
// kernel-provided child status through /proc; negligible setup cost.
fn probe_child_runs_with_no_new_privileges() {
    let result = qemu_output_from(
        "/bin/sh",
        &["-c", "grep '^NoNewPrivs:' /proc/self/status"],
        COMMAND_TIMEOUT,
    )
    .unwrap()
    .unwrap();
    assert_eq!(result, b"NoNewPrivs:\t1\n");
}

#[cfg(target_os = "linux")]
#[test]
// Production boundary: a continuously writable probe pipe cannot starve the
// deadline check. Uses only the POSIX shell builtin printf; setup is negligible.
fn timeout_applies_while_probe_stdout_is_continuously_readable() {
    let started = std::time::Instant::now();
    let result = qemu_output_from(
        "/bin/sh",
        &[
            "-c",
            "while :; do printf xxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxx; done",
        ],
        std::time::Duration::from_millis(100),
    );
    assert_eq!(result.unwrap_err().kind(), std::io::ErrorKind::TimedOut);
    assert!(started.elapsed() < std::time::Duration::from_secs(2));
}
