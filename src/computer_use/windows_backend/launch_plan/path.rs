#[cfg(windows)]
use std::path::PathBuf;
use std::{ffi::OsString, path::Path};

use super::WindowsLaunchBlocker;

pub(super) fn qemu_args(
    overlay: &Path,
    boot_config: &Path,
    pipe_name: &str,
    vcpus: u16,
    memory_bytes: u64,
) -> Result<Vec<OsString>, WindowsLaunchBlocker> {
    let overlay = qemu_option_path(overlay)?;
    let blockdev = serde_json::json!({
        "driver": "qcow2",
        "node-name": "guestdisk",
        "file": { "driver": "file", "filename": overlay },
        "cache": { "direct": false, "no-flush": false }
    });
    let blockdev =
        serde_json::to_string(&blockdev).map_err(|_| WindowsLaunchBlocker::InvalidOverlay)?;
    let boot = format!(
        "name=opt/org.koolade.computer-use/boot-config,file={}",
        qemu_option_path(boot_config)?
    );
    let vcpus = format!("cores={vcpus},threads=1,sockets=1");
    let memory = memory_bytes.to_string();
    let chardev = format!("pipe,id=guestcontrol,path={pipe_name}");
    let serial = "virtserialport,chardev=guestcontrol,name=org.koolade.computer-control.v1";
    Ok([
        "-no-user-config",
        "-nodefaults",
        "-accel",
        "whpx",
        "-machine",
        "q35",
        "-cpu",
        "host",
        "-smp",
        &vcpus,
        "-m",
        &memory,
        "-blockdev",
        &blockdev,
        "-device",
        "virtio-blk-pci,drive=guestdisk",
        "-device",
        "virtio-vga",
        "-display",
        "none",
        "-vnc",
        "none",
        "-nic",
        "none",
        "-monitor",
        "none",
        "-serial",
        "none",
        "-parallel",
        "none",
        "-chardev",
        &chardev,
        "-device",
        "virtio-serial-pci",
        "-device",
        serial,
        "-fw_cfg",
        &boot,
        "-no-reboot",
    ]
    .into_iter()
    .map(OsString::from)
    .collect())
}

pub(super) fn qemu_option_path(path: &Path) -> Result<String, WindowsLaunchBlocker> {
    let value = path.to_str().ok_or(WindowsLaunchBlocker::UnsafePath)?;
    if value.contains(',') || value.chars().any(char::is_control) {
        return Err(WindowsLaunchBlocker::UnsafePath);
    }
    Ok(value.to_owned())
}

pub(super) fn validate_qemu_path(path: &Path) -> Result<(), WindowsLaunchBlocker> {
    if !path.is_absolute() || contains_dot_component(path) || contains_reserved_component(path) {
        return Err(WindowsLaunchBlocker::UnsafePath);
    }
    qemu_option_path(path).map(|_| ())
}

fn contains_dot_component(path: &Path) -> bool {
    path.to_str().is_none_or(|value| {
        value
            .split(['/', '\\'])
            .any(|component| component == "." || component == "..")
    })
}

fn contains_reserved_component(path: &Path) -> bool {
    path.to_str().is_none_or(|value| {
        value.split(['/', '\\']).any(|component| {
            let base = component
                .split('.')
                .next()
                .unwrap_or_default()
                .trim_end_matches([' ', '.'])
                .to_ascii_uppercase();
            matches!(base.as_str(), "CON" | "PRN" | "AUX" | "NUL")
                || ["COM", "LPT"].iter().any(|prefix| {
                    base.strip_prefix(prefix).is_some_and(|suffix| {
                        suffix.len() == 1 && matches!(suffix.as_bytes()[0], b'1'..=b'9')
                    })
                })
        })
    })
}

#[cfg(windows)]
pub(super) fn validate_native_path_no_reparse(path: &Path) -> Result<(), WindowsLaunchBlocker> {
    use std::os::windows::fs::MetadataExt;
    use windows_sys::Win32::Storage::FileSystem::FILE_ATTRIBUTE_REPARSE_POINT;

    let mut current = PathBuf::new();
    for component in path.components() {
        current.push(component.as_os_str());
        if matches!(component, std::path::Component::Prefix(_)) {
            continue;
        }
        let metadata =
            std::fs::symlink_metadata(&current).map_err(|_| WindowsLaunchBlocker::UnsafePath)?;
        if metadata.file_attributes() & FILE_ATTRIBUTE_REPARSE_POINT != 0 {
            return Err(WindowsLaunchBlocker::UnsafePath);
        }
    }
    if !std::fs::metadata(path)
        .map_err(|_| WindowsLaunchBlocker::UnsafePath)?
        .is_file()
    {
        return Err(WindowsLaunchBlocker::UnsafePath);
    }
    Ok(())
}

pub(super) fn canonical_session_id(value: &str) -> bool {
    uuid::Uuid::parse_str(value).is_ok_and(|uuid| uuid.to_string() == value)
}
