use std::{
    io::{self, Read},
    time::Duration,
};

mod process;
use process::bounded_command_output;
#[cfg(test)]
use process::retain_bounded;

pub(super) const MAX_RETAINED_OUTPUT: usize = 4096;
const MAX_CPUINFO_BYTES: usize = 64 * 1024;
const COMMAND_TIMEOUT: Duration = Duration::from_secs(2);

pub(super) struct SystemProbe;

impl super::VmHostProbe for SystemProbe {
    fn cpu_virtualization_flags(&self) -> io::Result<Option<bool>> {
        cpu_virtualization_flags()
    }

    fn virtualization_present(&self) -> io::Result<Option<bool>> {
        virtualization_present()
    }

    fn open_kvm(&self) -> io::Result<()> {
        std::fs::OpenOptions::new()
            .read(true)
            .write(true)
            .open("/dev/kvm")
            .map(drop)
    }

    fn qemu_output(&self, arguments: &[&str]) -> io::Result<Option<Vec<u8>>> {
        qemu_output(arguments)
    }

    fn qemu_image_tool(&self) -> io::Result<()> {
        let path = "/usr/bin/qemu-img";
        std::fs::symlink_metadata(path)?;
        crate::computer_use::guest_image::probe_host_qemu_image_tool_for_readiness().map_err(
            |error| match error {
                crate::computer_use::guest_image::QemuProcessError::Unavailable => {
                    io::Error::new(io::ErrorKind::PermissionDenied, "untrusted qemu-img")
                }
                crate::computer_use::guest_image::QemuProcessError::TimedOut => io::Error::new(
                    io::ErrorKind::TimedOut,
                    "qemu-img readiness probe timed out",
                ),
                crate::computer_use::guest_image::QemuProcessError::OutputTooLarge
                | crate::computer_use::guest_image::QemuProcessError::Failed => {
                    io::Error::new(io::ErrorKind::InvalidData, "invalid qemu-img probe result")
                }
            },
        )
    }

    fn launcher_containment(&self) -> io::Result<()> {
        crate::computer_use::verify_qemu_launcher_containment_v1().map_err(|error| {
            use crate::computer_use::ComputerBackendErrorV1;
            match error {
                ComputerBackendErrorV1::QemuUnavailable => {
                    io::Error::new(io::ErrorKind::PermissionDenied, "untrusted QEMU launcher")
                }
                ComputerBackendErrorV1::UnsupportedHost => {
                    io::Error::new(io::ErrorKind::Unsupported, "unsupported QEMU launcher host")
                }
                _ => io::Error::other("QEMU launcher containment is unknown"),
            }
        })
    }
}

pub(super) fn cpu_virtualization_flags() -> io::Result<Option<bool>> {
    let mut file = std::fs::File::open("/proc/cpuinfo")?;
    let mut bytes = Vec::with_capacity(MAX_CPUINFO_BYTES);
    file.by_ref()
        .take((MAX_CPUINFO_BYTES + 1) as u64)
        .read_to_end(&mut bytes)?;
    if bytes.len() > MAX_CPUINFO_BYTES {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "CPU information exceeded the readiness probe bound",
        ));
    }
    let contents = std::str::from_utf8(&bytes)
        .map_err(|_| io::Error::new(io::ErrorKind::InvalidData, "CPU information was not UTF-8"))?;
    Ok(cpu_virtualization_flags_from_info(contents))
}

pub(super) fn virtualization_present() -> io::Result<Option<bool>> {
    let Some((status, output)) =
        bounded_command_output("/usr/bin/systemd-detect-virt", &["--vm"], COMMAND_TIMEOUT)?
    else {
        return Ok(None);
    };
    Ok(virtualization_from_status_and_output(
        status.code(),
        &output,
    ))
}

fn virtualization_from_status_and_output(code: Option<i32>, output: &[u8]) -> Option<bool> {
    let output = std::str::from_utf8(output).ok()?.trim();
    match (code, output) {
        (Some(0), value) if !value.is_empty() && value != "none" => Some(true),
        (Some(1), "none") => Some(false),
        _ => None,
    }
}

fn cpu_virtualization_flags_from_info(contents: &str) -> Option<bool> {
    let flags = contents.lines().find_map(|line| {
        let (name, value) = line.split_once(':')?;
        (name.trim() == "flags").then_some(value)
    })?;
    Some(
        flags
            .split_ascii_whitespace()
            .any(|flag| matches!(flag, "vmx" | "svm")),
    )
}

pub(super) fn qemu_output(arguments: &[&str]) -> io::Result<Option<Vec<u8>>> {
    qemu_output_from("/usr/bin/qemu-system-x86_64", arguments, COMMAND_TIMEOUT)
}

fn qemu_output_from(
    program: &str,
    arguments: &[&str],
    timeout: Duration,
) -> io::Result<Option<Vec<u8>>> {
    let Some((status, output)) = bounded_command_output(program, arguments, timeout)? else {
        return Ok(None);
    };
    Ok(status.success().then_some(output))
}

#[cfg(test)]
#[path = "probe_tests.rs"]
mod tests;
