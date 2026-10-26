use super::{EndpointObservation, ProbeOutcome};

#[cfg(target_os = "linux")]
pub(super) fn current_endpoint() -> EndpointObservation {
    use crate::execution::docker::engine::{DockerEndpoint, HostPlatform};
    EndpointObservation {
        trusted: DockerEndpoint::from_process_environment(HostPlatform::Linux)
            .is_ok_and(|endpoint| endpoint.supports_application_execution()),
        fingerprint: crate::execution::docker::environment::DockerEnvironmentStatus::current_endpoint_fingerprint(),
    }
}

#[cfg(not(target_os = "linux"))]
pub(super) fn current_endpoint() -> EndpointObservation {
    EndpointObservation {
        trusted: false,
        fingerprint: None,
    }
}

#[cfg(target_os = "linux")]
pub(super) fn rootlesskit_smoke() -> ProbeOutcome {
    use std::{
        process::{Command, Stdio},
        time::{Duration, Instant},
    };
    let uid = unsafe { libc::geteuid() };
    if uid == 0 {
        return ProbeOutcome::Unknown;
    }
    let Some(path) = trusted_binary(&["/usr/bin/rootlesskit", "/usr/sbin/rootlesskit"]) else {
        return ProbeOutcome::Unknown;
    };
    let Ok(home) = current_home(uid) else {
        return ProbeOutcome::Unknown;
    };
    let Ok(mut child) = Command::new(path)
        .arg("true")
        .env_clear()
        .env("HOME", home)
        .env("PATH", "/usr/sbin:/usr/bin:/sbin:/bin")
        .env("LANG", "C")
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
    else {
        return ProbeOutcome::Unknown;
    };
    let deadline = Instant::now() + Duration::from_secs(5);
    loop {
        match child.try_wait() {
            Ok(Some(status)) => {
                return if status.success() {
                    ProbeOutcome::Ready
                } else {
                    ProbeOutcome::Conflict
                };
            }
            Ok(None) if Instant::now() < deadline => std::thread::sleep(Duration::from_millis(10)),
            _ => {
                let _ = child.kill();
                let _ = child.wait();
                return ProbeOutcome::Unknown;
            }
        }
    }
}

#[cfg(not(target_os = "linux"))]
pub(super) fn rootlesskit_smoke() -> ProbeOutcome {
    ProbeOutcome::Unknown
}

#[cfg(target_os = "linux")]
pub(super) fn current_home(uid: u32) -> anyhow::Result<std::path::PathBuf> {
    use std::{
        ffi::CStr,
        os::unix::{ffi::OsStrExt, fs::MetadataExt},
        ptr,
    };
    let mut record = std::mem::MaybeUninit::<libc::passwd>::zeroed();
    let mut buffer = vec![0_i8; 16 * 1024];
    let mut result = ptr::null_mut();
    let status = unsafe {
        libc::getpwuid_r(
            uid,
            record.as_mut_ptr(),
            buffer.as_mut_ptr(),
            buffer.len(),
            &mut result,
        )
    };
    anyhow::ensure!(
        status == 0 && !result.is_null(),
        "current user's home is unavailable"
    );
    let record = unsafe { record.assume_init() };
    anyhow::ensure!(
        !record.pw_dir.is_null(),
        "current user's home is unavailable"
    );
    let home = std::path::PathBuf::from(std::ffi::OsStr::from_bytes(
        unsafe { CStr::from_ptr(record.pw_dir) }.to_bytes(),
    ));
    anyhow::ensure!(
        home.is_absolute() && std::fs::metadata(&home)?.uid() == uid,
        "current user's home is untrusted"
    );
    Ok(home)
}

#[cfg(target_os = "linux")]
pub(super) fn trusted_binary(paths: &[&str]) -> Option<std::path::PathBuf> {
    use std::os::unix::fs::MetadataExt;
    for path in paths {
        let Ok(canonical) = std::path::Path::new(path).canonicalize() else {
            continue;
        };
        let Ok(meta) = std::fs::metadata(&canonical) else {
            continue;
        };
        if meta.is_file() && meta.uid() == 0 && meta.mode() & 0o022 == 0 {
            return Some(canonical);
        }
    }
    None
}
