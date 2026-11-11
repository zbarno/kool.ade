use super::*;

pub(super) fn package_metadata_matches(
    path: &Path,
    expected_package: &str,
    expected_version: &str,
) -> bool {
    let mut command = Command::new("/usr/bin/dpkg-deb");
    command
        .arg("--field")
        .arg(path)
        .arg("Package")
        .env_clear()
        .stdin(Stdio::null())
        .stderr(Stdio::null());
    let Ok(package) = command.output() else {
        return false;
    };
    let mut version_command = Command::new("/usr/bin/dpkg-deb");
    version_command
        .arg("--field")
        .arg(path)
        .arg("Version")
        .env_clear()
        .stdin(Stdio::null())
        .stderr(Stdio::null());
    let Ok(version) = version_command.output() else {
        return false;
    };
    package.status.success()
        && version.status.success()
        && package.stdout == format!("{expected_package}\n").as_bytes()
        && version.stdout == format!("{expected_version}\n").as_bytes()
}

pub(super) fn installed_version(package_id: &str) -> Option<String> {
    let output = Command::new("/usr/bin/dpkg-query")
        .args(["-W", "-f=${Status}\n${Version}", package_id])
        .env_clear()
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }
    let text = String::from_utf8(output.stdout).ok()?;
    let (status, version) = text.split_once('\n')?;
    (status == "install ok installed").then(|| version.to_owned())
}

pub(crate) fn package_is_installed(
    package_id: &str,
    version: &str,
    rollback_version: Option<&str>,
) -> bool {
    installed_version(package_id).is_some_and(|installed| {
        installed == version || rollback_version == Some(installed.as_str())
    })
}

pub(super) fn cache_path(digest: &str) -> Option<PathBuf> {
    let hex = digest.strip_prefix("sha256:")?;
    if hex.len() != 64
        || !hex
            .bytes()
            .all(|byte| byte.is_ascii_hexdigit() && !byte.is_ascii_uppercase())
    {
        return None;
    }
    Some(Path::new(PACKAGE_CACHE).join(format!("{hex}.deb")))
}

pub(super) fn verify_cached_file(
    path: &Path,
    digest: &str,
    expected_uid: Option<u32>,
) -> Result<u64, &'static str> {
    let metadata = std::fs::symlink_metadata(path).map_err(|_| "cached app package unavailable")?;
    if !metadata.file_type().is_file()
        || expected_uid.is_some_and(|uid| metadata.uid() != uid)
        || metadata.mode() & 0o022 != 0
        || metadata.len() == 0
        || metadata.len() > MAX_PACKAGE_BYTES
    {
        return Err("cached app package is unsafe or oversized");
    }
    let mut options = OpenOptions::new();
    options
        .read(true)
        .custom_flags(libc::O_NOFOLLOW | libc::O_CLOEXEC);
    let mut file = options
        .open(path)
        .map_err(|_| "cached app package cannot be opened")?;
    let mut hasher = Sha256::new();
    let mut buffer = [0; 64 * 1024];
    loop {
        let count = file
            .read(&mut buffer)
            .map_err(|_| "cached app package cannot be read")?;
        if count == 0 {
            break;
        }
        hasher.update(&buffer[..count]);
    }
    let actual = format!("sha256:{:x}", hasher.finalize());
    if actual != digest {
        return Err("cached app package digest mismatch");
    }
    Ok(metadata.len())
}

pub(super) fn available_space(path: &Path) -> u64 {
    let Ok(path) = std::ffi::CString::new(path.as_os_str().as_encoded_bytes()) else {
        return 0;
    };
    let mut stats: libc::statvfs = unsafe { std::mem::zeroed() };
    if unsafe { libc::statvfs(path.as_ptr(), &mut stats) } != 0 {
        return 0;
    }
    (stats.f_bavail as u64).saturating_mul(stats.f_frsize as u64)
}

pub(super) fn has_operation_space(package_bytes: u64, available: u64) -> bool {
    let Some(required) = package_bytes
        .checked_mul(MAX_FREE_SPACE_MULTIPLIER)
        .and_then(|bytes| bytes.checked_add(MIN_FREE_SPACE))
    else {
        return false;
    };
    available >= required
}

pub(super) fn rollback_pair_present(version: Option<&str>, digest: Option<&str>) -> bool {
    version.is_some() && digest.is_some()
}
