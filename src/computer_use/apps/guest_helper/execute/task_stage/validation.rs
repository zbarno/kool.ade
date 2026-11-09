use super::*;
use std::os::unix::process::CommandExt;

pub(super) fn valid_action_scope(
    action: &ComputerActionV1,
    application: &StagedTaskApplicationV1,
    config: &crate::computer_use::apps::GuestAppHelperConfigV1,
    now: u64,
) -> bool {
    action.actor == ComputerActionActorV1::Human
        && action.operation == ComputerOperationV1::LaunchApprovedStagedTaskApp
        && action.session_id == config.session_id
        && action.lease_generation == config.lease_generation
        && action.approval_generation == config.approval_generation
        && action.cancellation_generation == config.cancellation_generation
        && action.action_sequence > config.initial_action_sequence
        && action.action_sequence <= 10_000
        && action.approval_generation > 0
        && action.cancellation_generation > 0
        && uuid::Uuid::parse_str(&action.idempotency_key)
            .is_ok_and(|id| id.to_string() == action.idempotency_key)
        && application.guest_image_digest == config.image_digest
        && config.session_class == crate::computer_use::ComputerSessionClassV1::Disposable
        && config
            .allowed_apps
            .iter()
            .any(|id| id == &application.app_id)
        && config.expires_at_unix_ms > now
}

pub(super) fn staged_manifest_matches(root: &Path, expected: &StagedTaskApplicationV1) -> bool {
    let path = root.join(".koolade-staged-app.json");
    let Ok(metadata) = fs::symlink_metadata(&path) else {
        return false;
    };
    use std::os::unix::fs::MetadataExt;
    if !metadata.file_type().is_file() || metadata.uid() != 0 || metadata.mode() & 0o222 != 0 {
        return false;
    }
    let Ok(bytes) = read_bounded(&path, 64 * 1024) else {
        return false;
    };
    serde_json::from_slice::<StagedTaskApplicationV1>(&bytes)
        .is_ok_and(|actual| actual == *expected)
}

pub(super) fn file_matches_receipt(path: &Path, expected_size: u64, expected_sha256: &str) -> bool {
    let Ok(bytes) = read_bounded(path, expected_size) else {
        return false;
    };
    bytes.len() as u64 == expected_size
        && format!("sha256:{}", hex_digest(&bytes)) == expected_sha256
}

pub(super) fn read_bounded(path: &Path, max_bytes: u64) -> Result<Vec<u8>, ()> {
    let mut options = OpenOptions::new();
    options
        .read(true)
        .custom_flags(libc::O_NOFOLLOW | libc::O_CLOEXEC);
    let file = options.open(path).map_err(|_| ())?;
    let metadata = file.metadata().map_err(|_| ())?;
    if !metadata.is_file() || metadata.len() > max_bytes || metadata.len() == 0 {
        return Err(());
    }
    let mut bytes = Vec::with_capacity(metadata.len() as usize);
    file.take(max_bytes.saturating_add(1))
        .read_to_end(&mut bytes)
        .map_err(|_| ())?;
    if bytes.len() as u64 > max_bytes {
        return Err(());
    }
    Ok(bytes)
}

pub(super) fn create_private_directory(path: &Path) -> std::io::Result<()> {
    fs::create_dir(path)?;
    fs::set_permissions(path, fs::Permissions::from_mode(0o700))?;
    Ok(())
}

pub(super) fn set_tree_read_only(root: &Path) -> bool {
    let Ok(entries) = fs::read_dir(root) else {
        return false;
    };
    for entry in entries {
        let Ok(entry) = entry else { return false };
        let path = entry.path();
        let Ok(metadata) = fs::symlink_metadata(&path) else {
            return false;
        };
        if metadata.is_dir()
            && (!set_tree_read_only(&path)
                || fs::set_permissions(&path, fs::Permissions::from_mode(0o555)).is_err())
        {
            return false;
        }
    }
    fs::set_permissions(root, fs::Permissions::from_mode(0o555)).is_ok()
}

pub(super) fn trusted_staged_executable(path: &Path, root: &Path) -> bool {
    let Ok(root) = root.canonicalize() else {
        return false;
    };
    let Ok(path) = path.canonicalize() else {
        return false;
    };
    if !path.starts_with(&root) {
        return false;
    }
    let Ok(metadata) = fs::symlink_metadata(path) else {
        return false;
    };
    use std::os::unix::fs::MetadataExt;
    metadata.is_file()
        && metadata.uid() == 0
        && metadata.mode() & 0o022 == 0
        && metadata.mode() & 0o111 != 0
}

pub(super) fn valid_elf_executable(bytes: &[u8], architecture: GuestAppArchitectureV1) -> bool {
    if bytes.len() < 20 || !bytes.starts_with(b"\x7fELF") || bytes[4] != 2 || bytes[5] != 1 {
        return false;
    }
    let elf_type = u16::from_le_bytes([bytes[16], bytes[17]]);
    let machine = u16::from_le_bytes([bytes[18], bytes[19]]);
    matches!(elf_type, 2 | 3)
        && match architecture {
            GuestAppArchitectureV1::X86_64 => machine == 62,
            GuestAppArchitectureV1::Aarch64 => machine == 183,
        }
}

pub(super) fn architecture_matches(architecture: GuestAppArchitectureV1) -> bool {
    matches!(
        (std::env::consts::ARCH, architecture),
        ("x86_64", GuestAppArchitectureV1::X86_64) | ("aarch64", GuestAppArchitectureV1::Aarch64)
    )
}

pub(super) fn has_stage_space(path: &Path, needed: u64) -> bool {
    let path = match std::ffi::CString::new(path.as_os_str().as_encoded_bytes()) {
        Ok(path) => path,
        Err(_) => return false,
    };
    let mut stats: libc::statvfs = unsafe { std::mem::zeroed() };
    (unsafe { libc::statvfs(path.as_ptr(), &mut stats) == 0 })
        && (stats.f_bavail as u64).saturating_mul(stats.f_frsize as u64)
            >= needed.saturating_add(128 * 1024 * 1024)
}

pub(super) fn session_root_path(action: &ComputerActionV1) -> PathBuf {
    Path::new(STAGING_ROOT).join(&action.session_id)
}

pub(super) fn create_launch_marker(session_root: &Path, operation_id: &str) -> bool {
    let path = session_root.join(format!(".launched-{operation_id}"));
    let mut options = OpenOptions::new();
    options
        .write(true)
        .create_new(true)
        .mode(0o400)
        .custom_flags(libc::O_NOFOLLOW | libc::O_CLOEXEC);
    options.open(path).is_ok()
}

pub(super) fn spawn_as_desktop(executable: &Path) -> ReplyV1 {
    let user = unsafe { libc::getpwnam(c"koolade".as_ptr()) };
    if user.is_null() {
        return ReplyV1::Unsupported;
    }
    let group_names = [c"audio", c"video", c"koolade-display"];
    let mut groups = Vec::with_capacity(group_names.len());
    for name in group_names {
        let group = unsafe { libc::getgrnam(name.as_ptr()) };
        if group.is_null() {
            return ReplyV1::Unsupported;
        }
        groups.push(unsafe { (*group).gr_gid });
    }
    let (uid, gid) = unsafe { ((*user).pw_uid, (*user).pw_gid) };
    let mut command = Command::new(executable);
    command
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
        Err(_) => ReplyV1::Failed,
    }
}

pub(super) fn unix_time_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis()
        .min(u64::MAX as u128) as u64
}

pub(super) fn hex_digest(bytes: &[u8]) -> String {
    use sha2::{Digest, Sha256};
    Sha256::digest(bytes)
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect()
}
