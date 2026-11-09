use super::*;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) enum DpkgOutcome {
    Completed,
    Failed,
    MissingDependencies,
    Interrupted,
}

pub(super) fn operate(
    manifest: &crate::computer_use::apps::GuestAppManifestV1,
    operation: GuestAppOperationKindV1,
    stream: &mut UnixStream,
) -> ReplyV1 {
    let Some(package_path) = cache_path(&manifest.package.package_sha256) else {
        return ReplyV1::Unsupported;
    };
    let Ok(package_size) =
        verify_cached_file(&package_path, &manifest.package.package_sha256, Some(0))
    else {
        return ReplyV1::Unsupported;
    };
    if !has_operation_space(package_size, available_space(Path::new("/"))) {
        return ReplyV1::Failed;
    }
    if !package_metadata_matches(
        &package_path,
        &manifest.package.package_id,
        &manifest.package.version,
    ) {
        return ReplyV1::Unsupported;
    }
    let installed = installed_version(&manifest.package.package_id);
    match operation {
        GuestAppOperationKindV1::Install if installed.is_some() => return ReplyV1::Unsupported,
        GuestAppOperationKindV1::Update => {
            if installed.as_deref() != manifest.package.rollback_version.as_deref() {
                return ReplyV1::Unsupported;
            }
        }
        GuestAppOperationKindV1::Remove
            if installed.as_deref() != Some(manifest.package.version.as_str())
                && installed.as_deref() != manifest.package.rollback_version.as_deref() =>
        {
            return ReplyV1::Unsupported;
        }
        GuestAppOperationKindV1::Launch => return ReplyV1::Unsupported,
        _ => {}
    }
    if operation == GuestAppOperationKindV1::Update {
        if !rollback_pair_present(
            manifest.package.rollback_version.as_deref(),
            manifest.package.rollback_package_sha256.as_deref(),
        ) {
            return ReplyV1::Unsupported;
        }
        let Some(rollback_digest) = manifest.package.rollback_package_sha256.as_deref() else {
            return ReplyV1::Unsupported;
        };
        let Some(rollback_path) = cache_path(rollback_digest) else {
            return ReplyV1::Unsupported;
        };
        let Ok(rollback_size) = verify_cached_file(&rollback_path, rollback_digest, Some(0)) else {
            return ReplyV1::Unsupported;
        };
        if !package_metadata_matches(
            &rollback_path,
            &manifest.package.package_id,
            manifest
                .package
                .rollback_version
                .as_deref()
                .unwrap_or_default(),
        ) || !has_operation_space(
            package_size.saturating_add(rollback_size),
            available_space(Path::new("/")),
        ) {
            return ReplyV1::Unsupported;
        }
        return update_package(
            &manifest.package.package_id,
            &package_path,
            &rollback_path,
            stream,
        );
    }
    match operation {
        GuestAppOperationKindV1::Install => {
            match run_dpkg(
                stream,
                ["--install".to_owned(), package_path.display().to_string()],
            ) {
                DpkgOutcome::Completed => ReplyV1::Completed,
                failure => {
                    let rollback = run_dpkg(
                        stream,
                        ["--remove".to_owned(), manifest.package.package_id.clone()],
                    );
                    if rollback == DpkgOutcome::Completed {
                        outcome_reply(failure)
                    } else {
                        ReplyV1::Failed
                    }
                }
            }
        }
        GuestAppOperationKindV1::Remove => {
            let (rollback_path, rollback_size) =
                if installed.as_deref() == manifest.package.rollback_version.as_deref() {
                    let Some(rollback_digest) = manifest.package.rollback_package_sha256.as_deref()
                    else {
                        return ReplyV1::Unsupported;
                    };
                    let Some(path) = cache_path(rollback_digest) else {
                        return ReplyV1::Unsupported;
                    };
                    let Ok(size) = verify_cached_file(&path, rollback_digest, Some(0)) else {
                        return ReplyV1::Unsupported;
                    };
                    (path, size)
                } else {
                    (package_path.clone(), package_size)
                };
            if !has_operation_space(rollback_size, available_space(Path::new("/"))) {
                return ReplyV1::Failed;
            }
            match run_dpkg(
                stream,
                ["--remove".to_owned(), manifest.package.package_id.clone()],
            ) {
                DpkgOutcome::Completed => ReplyV1::Completed,
                _ => {
                    let _ = run_dpkg(
                        stream,
                        ["--install".to_owned(), rollback_path.display().to_string()],
                    );
                    ReplyV1::Failed
                }
            }
        }
        GuestAppOperationKindV1::Update | GuestAppOperationKindV1::Launch => ReplyV1::Unsupported,
    }
}

fn update_package(
    package_id: &str,
    package_path: &Path,
    rollback_path: &Path,
    stream: &mut UnixStream,
) -> ReplyV1 {
    match run_dpkg(
        stream,
        ["--install".to_owned(), package_path.display().to_string()],
    ) {
        DpkgOutcome::Completed => ReplyV1::Completed,
        failure => {
            let rolled_back = run_dpkg(
                stream,
                ["--install".to_owned(), rollback_path.display().to_string()],
            );
            if rolled_back != DpkgOutcome::Completed {
                let cleanup = run_dpkg(stream, ["--remove".to_owned(), package_id.to_owned()]);
                return rollback_outcome(failure, cleanup);
            }
            rollback_outcome(failure, rolled_back)
        }
    }
}

fn outcome_reply(outcome: DpkgOutcome) -> ReplyV1 {
    match outcome {
        DpkgOutcome::MissingDependencies => ReplyV1::MissingDependencies,
        _ => ReplyV1::Failed,
    }
}

pub(super) fn rollback_outcome(primary: DpkgOutcome, rollback: DpkgOutcome) -> ReplyV1 {
    if rollback == DpkgOutcome::Completed {
        outcome_reply(primary)
    } else {
        ReplyV1::Failed
    }
}

fn run_dpkg<const N: usize>(stream: &mut UnixStream, args: [String; N]) -> DpkgOutcome {
    let mut command = Command::new("/usr/bin/dpkg");
    command
        .args(args)
        .env_clear()
        .env("PATH", "/usr/bin:/bin")
        .env("LANG", "C.UTF-8")
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::piped())
        .process_group(0);
    let Ok(mut child) = command.spawn() else {
        return DpkgOutcome::Failed;
    };
    let stderr = child.stderr.take();
    let capture = std::thread::spawn(move || bounded_capture(stderr));
    let pid = child.id() as libc::pid_t;
    let deadline = Instant::now() + COMMAND_TIMEOUT;
    loop {
        if let Ok(Some(status)) = child.try_wait() {
            unsafe { libc::kill(-pid, libc::SIGKILL) };
            let message = capture.join().unwrap_or_default();
            if status.success() {
                return DpkgOutcome::Completed;
            }
            return classify_dpkg_failure(&message);
        }
        if Instant::now() >= deadline || client_cancelled(stream) {
            unsafe { libc::kill(-pid, libc::SIGKILL) };
            let _ = child.wait();
            let _ = capture.join();
            return DpkgOutcome::Interrupted;
        }
    }
}

pub(super) fn classify_dpkg_failure(stderr: &[u8]) -> DpkgOutcome {
    let diagnostic = String::from_utf8_lossy(stderr).to_ascii_lowercase();
    if diagnostic.contains("dependency problems") || diagnostic.contains("unmet dependencies") {
        DpkgOutcome::MissingDependencies
    } else {
        DpkgOutcome::Failed
    }
}

fn bounded_capture(stderr: Option<std::process::ChildStderr>) -> Vec<u8> {
    let Some(mut stderr) = stderr else {
        return Vec::new();
    };
    let mut capture = Vec::with_capacity(MAX_DPKG_ERROR_BYTES);
    let mut buffer = [0; 1024];
    while let Ok(count) = stderr.read(&mut buffer) {
        if count == 0 {
            break;
        }
        let remaining = MAX_DPKG_ERROR_BYTES.saturating_sub(capture.len());
        capture.extend_from_slice(&buffer[..count.min(remaining)]);
    }
    capture
}

fn client_cancelled(stream: &UnixStream) -> bool {
    let mut descriptor = libc::pollfd {
        fd: stream.as_raw_fd(),
        events: libc::POLLIN,
        revents: 0,
    };
    let result = unsafe { libc::poll(&mut descriptor, 1, 100) };
    result < 0
        || descriptor.revents & (libc::POLLERR | libc::POLLHUP | libc::POLLNVAL | libc::POLLIN) != 0
}
