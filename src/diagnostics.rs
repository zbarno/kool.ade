//! Local reports for Rust panics and desktop startup failures.
//! Installation belongs in the executable, not the library or GUI frame loop.
use std::{
    fs,
    io::Write,
    path::{Path, PathBuf},
    sync::atomic::{AtomicU64, Ordering},
};

static NEXT_REPORT: AtomicU64 = AtomicU64::new(0);

pub fn install() {
    // Resolve once before installing the hook, avoiding configuration work in
    // the panic handler and keeping each process tied to its original state root.
    let directory = crate::persistence::state_root().join("crashes");
    let previous = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        let thread = std::thread::current();
        let detail = format!(
            "Thread: {}\n{info}\n\nBacktrace:\n{}\n",
            thread.name().unwrap_or("unnamed"),
            std::backtrace::Backtrace::force_capture()
        );
        match write_report(&directory, "Rust panic", &detail) {
            Ok(path) => eprintln!("Packet crash report: {}", path.display()),
            Err(error) => eprintln!("Could not save Packet crash report: {error}"),
        }
        previous(info);
    }));
}

pub fn record_startup_error(detail: &str) {
    let directory = crate::persistence::state_root().join("crashes");
    match write_report(&directory, "Desktop startup failure", detail) {
        Ok(path) => eprintln!("Packet startup failure report: {}", path.display()),
        Err(error) => eprintln!("Could not save Packet startup failure report: {error}"),
    }
}

fn write_report(directory: &Path, kind: &str, detail: &str) -> std::io::Result<PathBuf> {
    fs::create_dir_all(directory)?;
    let now = chrono::Utc::now();
    let path = directory.join(format!(
        "{}-{}-{}.txt",
        now.format("%Y%m%dT%H%M%S%.9fZ"),
        std::process::id(),
        NEXT_REPORT.fetch_add(1, Ordering::Relaxed)
    ));
    let mut options = fs::OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    let mut file = options.open(&path)?;
    writeln!(
        file,
        "Packet {}\nUTC: {now}\nPID: {}\nKind: {kind}\nExecutable: {}\n\n{detail}",
        env!("CARGO_PKG_VERSION"),
        std::process::id(),
        std::env::current_exe()
            .map(|path| path.display().to_string())
            .unwrap_or_default()
    )?;
    file.sync_all()?;
    Ok(path)
}

#[cfg(test)]
mod tests {
    #[test]
    fn panic_hook_saves_real_subprocess_backtrace() {
        const CHILD: &str = "PACKET_DIAGNOSTIC_TEST_CHILD";
        if std::env::var_os(CHILD).is_some() {
            super::install();
            panic!("diagnostic fixture panic");
        }
        let root = std::env::temp_dir().join(format!(
            "packet-crash-report-{}-{}",
            std::process::id(),
            chrono::Utc::now().timestamp_nanos_opt().unwrap()
        ));
        let output = std::process::Command::new(std::env::current_exe().unwrap())
            .args([
                "--exact",
                "diagnostics::tests::panic_hook_saves_real_subprocess_backtrace",
                "--nocapture",
            ])
            .env(CHILD, "1")
            .env("PACKET_HOME", &root)
            .output()
            .unwrap();
        assert!(!output.status.success());
        let reports = std::fs::read_dir(root.join("crashes"))
            .unwrap()
            .collect::<Result<Vec<_>, _>>()
            .unwrap();
        assert_eq!(reports.len(), 1);
        let report = std::fs::read_to_string(reports[0].path()).unwrap();
        assert!(report.contains("diagnostic fixture panic"));
        assert!(
            report.contains("Backtrace:")
                && report.contains("panic_hook_saves_real_subprocess_backtrace")
        );
        assert!(String::from_utf8_lossy(&output.stderr).contains("Packet crash report:"));
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            assert_eq!(
                std::fs::metadata(reports[0].path())
                    .unwrap()
                    .permissions()
                    .mode()
                    & 0o777,
                0o600
            );
        }
        std::fs::remove_dir_all(root).unwrap();
    }
}
