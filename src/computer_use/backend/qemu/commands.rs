use std::{ffi::OsString, path::Path, process::Command};

use crate::computer_use::{ComputerSessionV1, ComputerTargetV1};

use super::super::ComputerBackendErrorV1;

pub(super) fn command_args(
    session: &ComputerSessionV1,
    overlay: &Path,
    control_socket: &Path,
    display_socket: &Path,
    boot_config: &Path,
) -> Result<Vec<OsString>, ComputerBackendErrorV1> {
    let machine_id = match &session.target {
        ComputerTargetV1::LocalKvm { machine_id } => machine_id,
    };
    let budget = &session.policy.resource_budget;
    let vcpus = budget.cpu_millis.div_ceil(1_000).min(u16::MAX as u32);
    let memory_mib = budget.memory_bytes / (1024 * 1024);
    if vcpus == 0 || memory_mib == 0 {
        return Err(ComputerBackendErrorV1::ResourceLimit);
    }
    let identity = format!(
        "koolade:{machine_id}:{}:{}",
        session.identity.session_id, session.lease.lease_id
    );
    let smp = format!("{vcpus},sockets=1,cores={vcpus},threads=1");
    let memory = format!("size={memory_mib}M");
    let drive = format!(
        "if=none,id=guestdisk,file={},format=qcow2,cache=none,aio=threads,discard=unmap",
        overlay.display()
    );
    let chardev = format!(
        "socket,id=guestcontrol,path={},server=on,wait=off",
        control_socket.display()
    );
    let vnc = format!("unix:{},share=deny,clipboard=off", display_socket.display());
    let fw_cfg = format!(
        "name=opt/org.koolade.computer-use/boot-config,file={}",
        boot_config.display()
    );
    let name = format!("{identity},process={identity}");
    for path in [overlay, control_socket, display_socket, boot_config] {
        let text = path
            .to_str()
            .ok_or(ComputerBackendErrorV1::InvalidOverlay)?;
        if text.len() > 80
            || !text.starts_with('/')
            || !text.bytes().all(|byte| {
                byte.is_ascii_alphanumeric() || matches!(byte, b'/' | b'_' | b'-' | b'.' | b' ')
            })
        {
            return Err(ComputerBackendErrorV1::InvalidOverlay);
        }
    }

    let args = vec![
        "-name",
        &name,
        "-no-user-config",
        "-nodefaults",
        "-machine",
        "q35,accel=kvm",
        "-cpu",
        "host",
        "-smp",
        &smp,
        "-m",
        &memory,
        "-drive",
        &drive,
        "-device",
        "virtio-blk-pci,drive=guestdisk",
        "-device",
        "virtio-vga",
        "-display",
        "none",
        "-vnc",
        &vnc,
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
        "virtserialport,chardev=guestcontrol,name=org.koolade.computer-control.v1",
        "-fw_cfg",
        &fw_cfg,
        "-sandbox",
        "on,obsolete=deny,elevateprivileges=deny,spawn=deny,resourcecontrol=deny",
        "-overcommit",
        "mem-lock=off",
        "-no-reboot",
    ]
    .into_iter()
    .map(OsString::from)
    .collect::<Vec<_>>();
    Ok(args)
}

pub(super) fn configure_command(command: &mut Command, args: &[OsString], runtime_dir: &Path) {
    command
        .args(args)
        .current_dir(runtime_dir)
        .env_clear()
        .env("PATH", "/usr/bin:/bin")
        .env("HOME", "/nonexistent")
        .env("LC_ALL", "C")
        .env("TMPDIR", runtime_dir)
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null());
}
