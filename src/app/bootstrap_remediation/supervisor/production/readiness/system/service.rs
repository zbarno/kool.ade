use super::{ProbeOutcome, runtime};

#[cfg(target_os = "linux")]
mod unit;

#[cfg(target_os = "linux")]
pub(super) fn service_absent() -> ProbeOutcome {
    use std::{
        os::unix::fs::MetadataExt,
        path::PathBuf,
        process::{Command, Stdio},
        time::{Duration, Instant},
    };
    let uid = unsafe { libc::geteuid() };
    if uid == 0 {
        return ProbeOutcome::Unknown;
    }
    let Ok(home) = runtime::current_home(uid) else {
        return ProbeOutcome::Unknown;
    };
    let app_unit_directory = home.join(".config/systemd/user");
    let app_unit_path = app_unit_directory.join(unit::APP_UNIT_NAME);
    let mut app_unit_file = unit::UnitFileState::Missing;
    let units = [
        "docker.service",
        "docker.socket",
        "docker-rootless.service",
        "koolade-rootless-docker.service",
        "koolade-rootless-docker.socket",
    ];
    let dirs = [
        home.join(".config/systemd/user"),
        home.join(".local/share/systemd/user"),
        PathBuf::from("/etc/systemd/user"),
        PathBuf::from("/usr/local/lib/systemd/user"),
        PathBuf::from("/usr/lib/systemd/user"),
    ];
    for dir in dirs {
        let is_app_directory = dir == app_unit_directory;
        match safe_chain(&dir, if dir.starts_with(&home) { uid } else { 0 }, uid) {
            Entry::Missing => continue,
            Entry::Ready => (),
            Entry::Unknown => return ProbeOutcome::Unknown,
        }
        for unit in units {
            let path = dir.join(unit);
            if is_app_directory && unit == unit::APP_UNIT_NAME {
                match unit::inspect_app_unit(&path, uid) {
                    unit::UnitFileState::Missing => (),
                    unit::UnitFileState::Exact => app_unit_file = unit::UnitFileState::Exact,
                    unit::UnitFileState::Conflict => return ProbeOutcome::Conflict,
                    unit::UnitFileState::Unknown => return ProbeOutcome::Unknown,
                }
                continue;
            }
            match path_absent(&path) {
                ProbeOutcome::Ready => (),
                ProbeOutcome::Conflict => return ProbeOutcome::Conflict,
                ProbeOutcome::Unknown => return ProbeOutcome::Unknown,
            }
        }
    }
    for path in [
        PathBuf::from(format!("/run/user/{uid}/docker.sock")),
        PathBuf::from(format!("/run/user/{uid}/docker")),
        home.join(".local/share/docker"),
    ] {
        match path_absent(&path) {
            ProbeOutcome::Ready => (),
            ProbeOutcome::Conflict => return ProbeOutcome::Conflict,
            ProbeOutcome::Unknown => return ProbeOutcome::Unknown,
        }
    }
    match rootless_process(uid) {
        ProbeOutcome::Ready => (),
        other => return other,
    }
    let Some(systemctl) = runtime::trusted_binary(&["/usr/bin/systemctl", "/bin/systemctl"]) else {
        return ProbeOutcome::Unknown;
    };
    let runtime = format!("/run/user/{uid}");
    for unit in units {
        let Ok(mut child) = Command::new(&systemctl)
            .args([
                "--user",
                "show",
                "--no-pager",
                "--property=LoadState",
                "--property=ActiveState",
                "--property=FragmentPath",
                unit,
            ])
            .env_clear()
            .env("HOME", &home)
            .env("XDG_CONFIG_HOME", home.join(".config"))
            .env("XDG_RUNTIME_DIR", &runtime)
            .env(
                "DBUS_SESSION_BUS_ADDRESS",
                format!("unix:path={runtime}/bus"),
            )
            .env("PATH", "/usr/sbin:/usr/bin:/sbin:/bin")
            .env("LANG", "C")
            .env("LC_ALL", "C")
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn()
        else {
            return ProbeOutcome::Unknown;
        };
        let deadline = Instant::now() + Duration::from_secs(2);
        loop {
            match child.try_wait() {
                Ok(Some(_)) => break,
                Ok(None) if Instant::now() < deadline => {
                    std::thread::sleep(Duration::from_millis(10))
                }
                _ => {
                    let _ = child.kill();
                    let _ = child.wait();
                    return ProbeOutcome::Unknown;
                }
            }
        }
        let Ok(output) = child.wait_with_output() else {
            return ProbeOutcome::Unknown;
        };
        if !output.status.success() {
            return ProbeOutcome::Unknown;
        }
        let properties = String::from_utf8_lossy(&output.stdout);
        let Some(properties) = unit::parse_properties(&properties) else {
            return ProbeOutcome::Unknown;
        };
        let app_service = unit == unit::APP_UNIT_NAME;
        let retryable = if app_service {
            unit::resumable_app_unit(app_unit_file, &properties, &app_unit_path)
        } else {
            unit::retryable(&properties, false, &app_unit_path)
        };
        if !retryable {
            return ProbeOutcome::Conflict;
        }
    }
    if std::fs::metadata(&home).is_ok_and(|metadata| metadata.uid() == uid) {
        ProbeOutcome::Ready
    } else {
        ProbeOutcome::Unknown
    }
}

#[cfg(not(target_os = "linux"))]
pub(super) fn service_absent() -> ProbeOutcome {
    ProbeOutcome::Unknown
}

#[cfg(all(test, target_os = "linux"))]
#[path = "service/tests.rs"]
mod tests;

#[cfg(target_os = "linux")]
#[derive(Clone, Copy)]
enum Entry {
    Ready,
    Missing,
    Unknown,
}

#[cfg(target_os = "linux")]
fn safe_chain(path: &std::path::Path, owner: u32, uid: u32) -> Entry {
    use std::{os::unix::fs::MetadataExt, path::PathBuf};
    let mut current = PathBuf::from("/");
    for component in path.components().skip(1) {
        current.push(component);
        let meta = match std::fs::symlink_metadata(&current) {
            Ok(meta) => meta,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Entry::Missing,
            Err(_) => return Entry::Unknown,
        };
        if !meta.is_dir() || meta.file_type().is_symlink() || meta.mode() & 0o022 != 0 {
            return Entry::Unknown;
        }
        let last = current == path;
        if (last && meta.uid() != owner) || (!last && meta.uid() != 0 && meta.uid() != uid) {
            return Entry::Unknown;
        }
    }
    Entry::Ready
}

#[cfg(target_os = "linux")]
fn rootless_process(uid: u32) -> ProbeOutcome {
    use std::os::unix::fs::MetadataExt;
    let Ok(entries) = std::fs::read_dir("/proc") else {
        return ProbeOutcome::Unknown;
    };
    for entry in entries {
        let Ok(entry) = entry else {
            return ProbeOutcome::Unknown;
        };
        if !entry
            .file_name()
            .to_string_lossy()
            .bytes()
            .all(|byte| byte.is_ascii_digit())
        {
            continue;
        }
        let meta = match entry.metadata() {
            Ok(meta) => meta,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => continue,
            Err(_) => return ProbeOutcome::Unknown,
        };
        if meta.uid() != uid {
            continue;
        }
        let cmd = match std::fs::read(entry.path().join("cmdline")) {
            Ok(cmd) => cmd,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => continue,
            Err(_) => return ProbeOutcome::Unknown,
        };
        if command_line_is_rootless(&cmd) {
            return ProbeOutcome::Conflict;
        }
    }
    ProbeOutcome::Ready
}

#[cfg(target_os = "linux")]
fn path_absent(path: &std::path::Path) -> ProbeOutcome {
    match std::fs::symlink_metadata(path) {
        Ok(_) => ProbeOutcome::Conflict,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => ProbeOutcome::Ready,
        Err(_) => ProbeOutcome::Unknown,
    }
}

fn command_line_is_rootless(command_line: &[u8]) -> bool {
    let has = |needle: &[u8]| {
        command_line
            .windows(needle.len())
            .any(|window| window == needle)
    };
    has(b"dockerd-rootless") || has(b"--rootless") || (has(b"rootlesskit") && has(b"dockerd"))
}
