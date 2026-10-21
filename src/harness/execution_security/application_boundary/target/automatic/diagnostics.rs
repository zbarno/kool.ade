#[cfg(test)]
pub(super) fn retry_test_diagnostic(stage: impl FnOnce() -> String) {
    let Some(path) = std::env::var_os("KOOLADE_TEST_DIAGNOSTICS_FILE") else {
        return;
    };
    use std::io::Write;
    if let Ok(mut file) = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(path)
    {
        let _ = writeln!(file, "worker-retry E2E: {}", stage());
    }
}

#[cfg(not(test))]
pub(super) fn retry_test_diagnostic(_: impl FnOnce() -> String) {}
