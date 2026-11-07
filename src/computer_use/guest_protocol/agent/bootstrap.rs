use super::{GuestBootConfigV1, GuestProtocolErrorV1};
use std::{
    fs::{self, File, OpenOptions},
    io::Read,
    os::unix::fs::{MetadataExt, OpenOptionsExt, PermissionsExt},
    path::Path,
};
use zeroize::Zeroizing;

const FW_CFG_BOOT_CONFIG: &str =
    "/sys/firmware/qemu_fw_cfg/by_name/opt/org.koolade.computer-use/boot-config/raw";
const MAX_BOOT_CONFIG_BYTES: u64 = 4096;

pub(super) fn read_boot_config(path: &Path) -> Result<GuestBootConfigV1, GuestProtocolErrorV1> {
    read_config_for_uid(path, unsafe { libc::geteuid() })
}

fn read_config_for_uid(
    path: &Path,
    expected_uid: u32,
) -> Result<GuestBootConfigV1, GuestProtocolErrorV1> {
    let metadata = fs::symlink_metadata(path).map_err(|_| GuestProtocolErrorV1::Io)?;
    if !metadata.file_type().is_file()
        || metadata.uid() != expected_uid
        || metadata.permissions().mode() & 0o077 != 0
    {
        return Err(GuestProtocolErrorV1::WrongSession);
    }
    let mut bytes = Zeroizing::new(Vec::new());
    let mut options = OpenOptions::new();
    options
        .read(true)
        .custom_flags(libc::O_NOFOLLOW | libc::O_CLOEXEC);
    options
        .open(path)
        .and_then(|file| file.take(MAX_BOOT_CONFIG_BYTES + 1).read_to_end(&mut bytes))
        .map_err(|_| GuestProtocolErrorV1::Io)?;
    if bytes.is_empty() || bytes.len() as u64 > MAX_BOOT_CONFIG_BYTES {
        return Err(GuestProtocolErrorV1::Oversized);
    }
    let config: GuestBootConfigV1 =
        serde_json::from_slice(&bytes).map_err(|_| GuestProtocolErrorV1::Malformed)?;
    config
        .validate()
        .map_err(|_| GuestProtocolErrorV1::WrongSession)?;
    Ok(config)
}

pub(super) fn install_boot_config() -> Result<(), GuestProtocolErrorV1> {
    if unsafe { libc::geteuid() } != 0 {
        return Err(GuestProtocolErrorV1::WrongSession);
    }
    let mut bytes = Zeroizing::new(Vec::new());
    File::open(FW_CFG_BOOT_CONFIG)
        .and_then(|file| file.take(MAX_BOOT_CONFIG_BYTES + 1).read_to_end(&mut bytes))
        .map_err(|_| GuestProtocolErrorV1::Io)?;
    if bytes.is_empty() || bytes.len() as u64 > MAX_BOOT_CONFIG_BYTES {
        return Err(GuestProtocolErrorV1::Oversized);
    }
    let config: GuestBootConfigV1 =
        serde_json::from_slice(&bytes).map_err(|_| GuestProtocolErrorV1::Malformed)?;
    config
        .validate()
        .map_err(|_| GuestProtocolErrorV1::WrongSession)?;
    let directory = Path::new("/run/koolade-computer-use");
    fs::create_dir(directory)
        .or_else(|error| {
            if error.kind() == std::io::ErrorKind::AlreadyExists {
                Ok(())
            } else {
                Err(error)
            }
        })
        .map_err(|_| GuestProtocolErrorV1::Io)?;
    let metadata = fs::symlink_metadata(directory).map_err(|_| GuestProtocolErrorV1::Io)?;
    if !metadata.file_type().is_dir() || metadata.uid() != 0 {
        return Err(GuestProtocolErrorV1::WrongSession);
    }
    fs::set_permissions(directory, fs::Permissions::from_mode(0o755))
        .map_err(|_| GuestProtocolErrorV1::Io)?;
    let path = directory.join("boot.json");
    let mut options = OpenOptions::new();
    options.write(true).create_new(true);
    options
        .mode(0o400)
        .custom_flags(libc::O_NOFOLLOW | libc::O_CLOEXEC);
    let mut output = match options.open(&path) {
        Ok(file) => file,
        Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => {
            let user = unsafe { libc::getpwnam(c"koolade".as_ptr()) };
            if user.is_null() {
                return Err(GuestProtocolErrorV1::WrongSession);
            }
            return read_config_for_uid(&path, unsafe { (*user).pw_uid }).and_then(|existing| {
                if existing == config {
                    Ok(())
                } else {
                    Err(GuestProtocolErrorV1::WrongSession)
                }
            });
        }
        Err(_) => return Err(GuestProtocolErrorV1::Io),
    };
    let user = unsafe { libc::getpwnam(c"koolade".as_ptr()) };
    if user.is_null()
        || unsafe {
            libc::fchown(
                std::os::fd::AsRawFd::as_raw_fd(&output),
                (*user).pw_uid,
                (*user).pw_gid,
            )
        } != 0
    {
        return Err(GuestProtocolErrorV1::Io);
    }
    use std::io::Write;
    output
        .write_all(&bytes)
        .map_err(|_| GuestProtocolErrorV1::Io)?;
    output.sync_all().map_err(|_| GuestProtocolErrorV1::Io)
}
