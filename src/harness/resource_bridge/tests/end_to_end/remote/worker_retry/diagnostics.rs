pub(super) fn report_stage(stage: &str) {
    eprintln!("worker-retry E2E: {stage}");
    let Some(path) = std::env::var_os("KOOLADE_TEST_DIAGNOSTICS_FILE") else {
        return;
    };
    if let Ok(mut file) = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(path)
    {
        use std::io::Write;
        let _ = writeln!(file, "worker-retry E2E: {stage}");
    }
}
