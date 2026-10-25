use std::process::Command;

/// Production boundary: launches the compiled desktop executable on the
/// runner's native OS. It proves argument dispatch exits before GUI startup;
/// a unit test cannot verify the separate binary target. No external service
/// is needed, and setup cost is low.
#[test]
fn native_executable_reports_its_version_without_opening_a_window() {
    let output = Command::new(env!("CARGO_BIN_EXE_koolade"))
        .arg("--version")
        .output()
        .expect("start the native Kool.ad/e executable");

    assert!(output.status.success());
    assert_eq!(
        String::from_utf8(output.stdout).unwrap().trim(),
        format!("koolade {}", env!("CARGO_PKG_VERSION"))
    );
}

/// Production boundary: checks the same native executable and headless
/// startup path. It verifies the packaged CLI documents desktop and worker
/// entry points without initializing the windowing system. No external
/// service is needed, and setup cost is low.
#[test]
fn native_executable_prints_help_without_opening_a_window() {
    let output = Command::new(env!("CARGO_BIN_EXE_koolade"))
        .arg("--help")
        .output()
        .expect("start the native Kool.ad/e executable");
    let stdout = String::from_utf8(output.stdout).unwrap();

    assert!(output.status.success());
    assert!(stdout.contains("Usage: koolade"));
    assert!(stdout.contains("worker serve --stdio"));
}
