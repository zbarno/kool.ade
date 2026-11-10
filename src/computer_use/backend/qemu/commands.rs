use std::{ffi::OsString, net::SocketAddr, path::Path, process::Command};

use crate::computer_use::ComputerSessionV1;

use super::super::ComputerBackendErrorV1;

#[allow(clippy::too_many_arguments)] // Each path is validated and emitted as a separate QEMU asset.
pub(super) fn command_args(
    session: &ComputerSessionV1,
    overlay: &Path,
    control_socket: &Path,
    display_socket: &Path,
    boot_config: &Path,
    app_helper_config: &Path,
    application_catalog: Option<&Path>,
    research_proxy: Option<SocketAddr>,
    secret_fd: Option<libc::c_int>,
) -> Result<Vec<OsString>, ComputerBackendErrorV1> {
    use crate::computer_use::{ComputerCapabilityV1, ComputerNetworkProfileV1};
    if secret_fd.is_some()
        != (session.session_class
            == crate::computer_use::ComputerSessionClassV1::PersonalPersistent)
        || (secret_fd.is_some()
            && (session.policy.capability != ComputerCapabilityV1::PersistentPersonal
                || session.network_profile != ComputerNetworkProfileV1::Disabled
                || session.policy.network_profile != ComputerNetworkProfileV1::Disabled))
    {
        return Err(ComputerBackendErrorV1::UnsupportedPolicy);
    }
    let machine_id = session.target.target_machine_id();
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
    let drive = if secret_fd.is_some() {
        format!(
            "if=none,id=guestdisk,file={},format=qcow2,cache=none,aio=threads,discard=unmap,encrypt.key-secret=personal_vm_key",
            overlay.display()
        )
    } else {
        format!(
            "if=none,id=guestdisk,file={},format=qcow2,cache=none,aio=threads,discard=unmap",
            overlay.display()
        )
    };
    let secret_object =
        secret_fd.map(|fd| format!("secret,id=personal_vm_key,file=/proc/self/fd/{fd}"));
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
    let nic = match (session.network_profile, research_proxy) {
        (ComputerNetworkProfileV1::Disabled, None) => "none".to_owned(),
        (ComputerNetworkProfileV1::InternetResearch, Some(proxy))
            if proxy.ip().is_loopback()
                && proxy.port() != 0
                && session.policy.network_profile == ComputerNetworkProfileV1::InternetResearch
                && session.policy.capability == ComputerCapabilityV1::InternetResearch =>
        {
            format!(
                "user,id=researchnet,ipv4=on,ipv6=off,restrict=on,guestfwd=tcp:10.0.2.100:3128-tcp:127.0.0.1:{}",
                proxy.port()
            )
        }
        _ => return Err(ComputerBackendErrorV1::UnsupportedPolicy),
    };
    let helper_fw_cfg = format!(
        "name=opt/org.koolade.computer-use/app-helper-config,file={}",
        app_helper_config.display()
    );
    for path in [
        overlay,
        control_socket,
        display_socket,
        boot_config,
        app_helper_config,
    ]
    .into_iter()
    .chain(application_catalog)
    {
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

    let mut args = vec![
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
        &nic,
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
        "-fw_cfg",
        &helper_fw_cfg,
        "-sandbox",
        "on,obsolete=deny,elevateprivileges=deny,spawn=deny,resourcecontrol=deny",
        "-overcommit",
        "mem-lock=off",
        "-no-reboot",
    ]
    .into_iter()
    .map(OsString::from)
    .collect::<Vec<_>>();
    if let Some(secret_object) = secret_object {
        let object_position = args
            .iter()
            .position(|arg| arg == "-drive")
            .ok_or(ComputerBackendErrorV1::UnsupportedPolicy)?;
        args.splice(
            object_position..object_position,
            [OsString::from("-object"), OsString::from(secret_object)],
        );
    }
    if let Some(catalog) = application_catalog {
        let fw_cfg = format!(
            "name=opt/org.koolade.computer-use/app-catalog,file={}",
            catalog.display()
        );
        args.push(OsString::from("-fw_cfg"));
        args.push(OsString::from(fw_cfg));
    }
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
