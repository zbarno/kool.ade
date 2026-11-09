mod catalog;
mod package;
mod task_stage;

use std::{
    os::unix::process::CommandExt,
    path::Path,
    process::{Command, Stdio},
};

use crate::computer_use::{ComputerActionPayloadV1, ComputerActionV1};

use super::protocol::ReplyV1;

pub(super) fn begin_guest_helper_boot() -> Result<(), &'static str> {
    catalog::begin_guest_helper_boot()
}

pub(super) fn launch_as_desktop_user(app_id: &str) -> ReplyV1 {
    let Ok(config) = catalog::helper_config() else {
        return ReplyV1::Unsupported;
    };
    if !config.allowed_apps.iter().any(|allowed| allowed == app_id) {
        return ReplyV1::Unsupported;
    }
    let executable_id = match app_id {
        "chromium" | "file-manager" | "text-editor" | "test-gui-fixture" => app_id.to_owned(),
        _ => {
            let Ok(catalog) = catalog::verified_catalog() else {
                return ReplyV1::Unsupported;
            };
            let Some(manifest) = catalog.app(app_id) else {
                return ReplyV1::Unsupported;
            };
            if !package::package_is_installed(
                &manifest.package.package_id,
                &manifest.package.version,
                manifest.package.rollback_version.as_deref(),
            ) {
                return ReplyV1::Unsupported;
            }
            manifest.executable_id.clone()
        }
    };
    let (path, args) = match executable_id.as_str() {
        "chromium" => (
            "/usr/bin/chromium",
            vec![
                "--no-first-run",
                "--no-default-browser-check",
                "--disable-sync",
                "--disable-background-networking",
                "--user-data-dir=/home/koolade/.cache/koolade-chromium",
                "--disk-cache-dir=/home/koolade/.cache/koolade-chromium-cache",
                "about:blank",
            ],
        ),
        "file-manager" => ("/usr/bin/thunar", vec!["--no-desktop"]),
        "text-editor" => ("/usr/bin/mousepad", vec!["--new-window"]),
        "test-gui-fixture" => ("/usr/lib/koolade/computer-use-test-fixture", vec![]),
        _ => return ReplyV1::Unsupported,
    };
    if !trusted_executable_present(path) {
        return ReplyV1::Unsupported;
    }
    let user = unsafe { libc::getpwnam(c"koolade".as_ptr()) };
    let group_names = [c"audio", c"video", c"koolade-display"];
    let mut groups = Vec::with_capacity(group_names.len());
    for name in group_names {
        let group = unsafe { libc::getgrnam(name.as_ptr()) };
        if group.is_null() {
            return ReplyV1::Unsupported;
        }
        groups.push(unsafe { (*group).gr_gid });
    }
    if user.is_null() {
        return ReplyV1::Unsupported;
    }
    let (uid, gid) = unsafe { ((*user).pw_uid, (*user).pw_gid) };
    let mut command = Command::new(path);
    command
        .args(args)
        .env_clear()
        .env("PATH", "/usr/bin:/bin")
        .env("HOME", "/home/koolade")
        .env("DISPLAY", ":0")
        .env("XAUTHORITY", "/home/koolade/.Xauthority")
        .env("LANG", "C.UTF-8")
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null());
    let groups_for_child = groups.clone();
    unsafe {
        command.pre_exec(move || {
            if libc::setgroups(groups_for_child.len(), groups_for_child.as_ptr()) != 0 {
                return Err(std::io::Error::last_os_error());
            }
            if libc::setgid(gid) != 0 || libc::setuid(uid) != 0 {
                return Err(std::io::Error::last_os_error());
            }
            Ok(())
        });
    }
    match command.spawn() {
        Ok(mut child) => {
            std::thread::spawn(move || {
                let _ = child.wait();
            });
            ReplyV1::Completed
        }
        Err(_) => ReplyV1::Unsupported,
    }
}

pub(super) fn manage(
    action: &ComputerActionV1,
    stream: &mut std::os::unix::net::UnixStream,
) -> ReplyV1 {
    let ComputerActionPayloadV1::ManageApp { .. } = action.payload else {
        return ReplyV1::Unsupported;
    };
    package::manage(action, stream)
}

pub(super) fn status() -> ReplyV1 {
    catalog::status()
}

pub(super) fn begin_task_stage(
    action: &ComputerActionV1,
) -> Result<task_stage::PendingTaskStage, ReplyV1> {
    task_stage::PendingTaskStage::begin(action)
}

pub(super) fn write_task_stage_chunk(
    stage: &mut task_stage::PendingTaskStage,
    artifact_index: u16,
    offset: u64,
    bytes: &[u8],
) -> ReplyV1 {
    stage.write_chunk(artifact_index, offset, bytes)
}

pub(super) fn finish_task_stage(stage: task_stage::PendingTaskStage) -> ReplyV1 {
    stage.finish()
}

pub(super) fn launch_staged_task(action: &ComputerActionV1) -> ReplyV1 {
    task_stage::launch(action)
}

fn trusted_executable_present(path: &str) -> bool {
    let Ok(path) = Path::new(path).canonicalize() else {
        return false;
    };
    if !path.starts_with("/usr") {
        return false;
    }
    let Ok(metadata) = std::fs::metadata(path) else {
        return false;
    };
    use std::os::unix::fs::MetadataExt;
    metadata.is_file()
        && metadata.uid() == 0
        && metadata.mode() & 0o022 == 0
        && metadata.mode() & 0o111 != 0
}
