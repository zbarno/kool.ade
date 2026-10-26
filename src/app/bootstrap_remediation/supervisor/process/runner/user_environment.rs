use std::path::PathBuf;

pub(super) struct UserEnvironment {
    pub(super) home: PathBuf,
    pub(super) runtime_dir: PathBuf,
}

#[cfg(target_os = "linux")]
pub(super) fn current_user_systemd() -> anyhow::Result<UserEnvironment> {
    use std::os::unix::fs::{FileTypeExt, MetadataExt};

    let uid = unsafe { libc::geteuid() };
    anyhow::ensure!(uid != 0, "rootless service operations cannot run as root");
    let home = current_user_home(uid)?.canonicalize()?;
    validate_config_home(&home, std::env::var_os("XDG_CONFIG_HOME").as_deref())?;
    #[cfg(target_os = "linux")]
    drop(super::home_chain::open_trusted_home(&home, uid)?);
    let home_metadata = std::fs::symlink_metadata(&home)?;
    anyhow::ensure!(
        home_metadata.is_dir()
            && !home_metadata.file_type().is_symlink()
            && home_metadata.uid() == uid
            && home_metadata.mode() & 0o022 == 0,
        "current user's home directory is not a trusted writable directory"
    );

    let runtime_dir = PathBuf::from(format!("/run/user/{uid}"));
    let runtime_metadata = std::fs::symlink_metadata(&runtime_dir)?;
    anyhow::ensure!(
        runtime_metadata.is_dir()
            && !runtime_metadata.file_type().is_symlink()
            && runtime_metadata.uid() == uid
            && runtime_metadata.mode() & 0o777 == 0o700,
        "current user's systemd runtime directory is unavailable or unsafe"
    );
    for socket in [runtime_dir.join("systemd/private"), runtime_dir.join("bus")] {
        let metadata = std::fs::symlink_metadata(socket)?;
        anyhow::ensure!(
            metadata.file_type().is_socket()
                && metadata.uid() == uid
                && metadata.mode() & 0o022 == 0,
            "current user's systemd socket is unavailable or unsafe"
        );
    }
    Ok(UserEnvironment { home, runtime_dir })
}

#[cfg(not(target_os = "linux"))]
pub(super) fn current_user_systemd() -> anyhow::Result<UserEnvironment> {
    anyhow::bail!("current-user systemd operations are Linux-only")
}

fn validate_config_home(
    home: &std::path::Path,
    configured: Option<&std::ffi::OsStr>,
) -> anyhow::Result<()> {
    let expected = home.join(".config");
    if let Some(configured) = configured {
        let configured = std::path::Path::new(configured);
        anyhow::ensure!(
            configured.is_absolute() && configured == expected,
            "custom XDG_CONFIG_HOME is unsupported for the rootless user service"
        );
    }
    Ok(())
}

#[cfg(target_os = "linux")]
fn current_user_home(uid: u32) -> anyhow::Result<PathBuf> {
    use std::{ffi::CStr, os::unix::ffi::OsStrExt, ptr};

    let mut record = std::mem::MaybeUninit::<libc::passwd>::zeroed();
    let mut buffer = vec![0 as libc::c_char; 16 * 1024];
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
        "current user's home directory could not be resolved"
    );
    let record = unsafe { record.assume_init() };
    anyhow::ensure!(
        !record.pw_dir.is_null(),
        "current user's home is unavailable"
    );
    let path = unsafe { CStr::from_ptr(record.pw_dir) }.to_bytes();
    let home = PathBuf::from(std::ffi::OsStr::from_bytes(path));
    anyhow::ensure!(home.is_absolute(), "current user's home is not absolute");
    Ok(home)
}

#[cfg(not(target_os = "linux"))]
fn current_user_home(_uid: u32) -> anyhow::Result<PathBuf> {
    anyhow::bail!("current-user systemd operations are Linux-only")
}

#[cfg(test)]
mod tests {
    use super::validate_config_home;
    use std::{ffi::OsStr, path::Path};

    #[test]
    fn rootless_service_config_home_accepts_only_the_default_xdg_location() {
        let home = Path::new("/home/synthetic-user");
        assert!(validate_config_home(home, None).is_ok());
        assert!(
            validate_config_home(home, Some(OsStr::new("/home/synthetic-user/.config"))).is_ok()
        );
        assert!(validate_config_home(home, Some(OsStr::new("/srv/config"))).is_err());
        assert!(validate_config_home(home, Some(OsStr::new("relative/config"))).is_err());
    }
}
