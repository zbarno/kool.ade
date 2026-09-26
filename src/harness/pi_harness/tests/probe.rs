use super::super::*;

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
    assert_eq!(
        stall_timeout_from_raw(Some(" 120 ")),
        Duration::from_secs(120)
    );
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
