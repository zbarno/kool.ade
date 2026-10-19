use super::super::*;

#[test]
fn version_extraction_handles_shapes() {
    assert_eq!(extract_version("pi - v0.84.4"), "0.84.4");
    assert_eq!(extract_version("0.84.4"), "0.84.4");
    assert_eq!(extract_version("weird output"), "");
    assert_eq!(
        extract_version("pi 0.84.4 token=synthetic-secret"),
        "0.84.4"
    );
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
    let mut environment = crate::test_support::ProcessEnvironmentGuard::new();
    environment.set(PI_BINARY_ENV, "/definitely/not/a/pi/binary");
    let res = PiHarness::locate_binary();
    assert!(matches!(res, Err(AppError::HarnessNotFound { .. })));
}

/// A bogus-but-set override FAILS FAST through the report path: no
/// binary, no fall-through claim, the exact coarse status line, and the
/// actionable override text in `diagnostic`. Parity invariant: under the
/// same forced state the report status is byte-identical to `label()`.

#[test]
fn probe_report_bogus_override_fast_fails_with_label_parity() {
    let mut environment = crate::test_support::ProcessEnvironmentGuard::new();
    environment.set(PI_BINARY_ENV, "/nonexistent-koolade-selftest/pi");
    let rep = PiHarness::probe_report();
    // Trait label under the IDENTICALLY forced state (captured before
    // restoring so the parity comparison spans one environment state).
    let label = (<PiHarness as AiHarness>::label)(&PiHarness);
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
    let mut environment = crate::test_support::ProcessEnvironmentGuard::new();
    let prev_path = std::env::var_os("PATH");
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
    environment.set("HOME", "/nonexistent-koolade-selftest-home");
    environment.set("PATH", narrowed);
    environment.remove(PI_BINARY_ENV);
    let rep = PiHarness::probe_report();
    assert!(!rep.ok, "undiscoverable pi must not report ok");
    assert!(rep.binary.is_none(), "no binary may win: {:?}", rep.binary);
    assert_eq!(rep.status, "pi (unavailable: Pi harness not found)");
    assert!(
        rep.diagnostic.contains("set KOOLADE_PI_BIN to override"),
        "diagnostic must name the override remedy: {}",
        rep.diagnostic
    );
}

/// Host-agnostic invariant (green on pi-installed and pi-less CI alike):
/// status always keeps the label shape, `ok` agrees with the binary and
/// the "(unavailable" marker, and a reported binary is a real file.

#[test]
fn probe_report_invariants_hold_on_any_host() {
    let _environment_lock = crate::test_support::lock_process_environment();
    let rep = PiHarness::probe_report();
    assert!(rep.status.starts_with("pi "), "status: {}", rep.status);
    if rep.configuration_required {
        assert!(!rep.ok, "missing provider configuration is not ready");
        assert!(rep.binary.is_some(), "Pi must be discoverable");
        assert!(
            !rep.status.contains("(unavailable"),
            "status: {}",
            rep.status
        );
        assert!(
            !rep.diagnostic.is_empty(),
            "missing configuration needs guidance"
        );
    } else if rep.ok {
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
