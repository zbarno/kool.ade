use super::super::{Check, QemuCapabilities, VmFacts};
use super::{host_distribution_check, host_evidence_check, host_user_namespaces_check};
use crate::app::setup_attention::catalog::{HostKind, ImageArchitectureV1};

pub(in crate::app::setup_attention::scan::vm_readiness) fn collect(
    host: HostKind,
    observed_architecture: ImageArchitectureV1,
    evidence: &[crate::app::setup_attention::catalog::ProbeEvidenceV1],
    generation: u64,
    resources: super::super::resources::ResourceCapacity,
) -> VmFacts {
    collect_with(
        host,
        observed_architecture,
        evidence,
        generation,
        resources,
        &super::super::probe::SystemProbe,
    )
}

pub(super) fn collect_with(
    host: HostKind,
    observed_architecture: ImageArchitectureV1,
    evidence: &[crate::app::setup_attention::catalog::ProbeEvidenceV1],
    generation: u64,
    resources: super::super::resources::ResourceCapacity,
    probe: &impl super::super::VmHostProbe,
) -> VmFacts {
    if host != HostKind::Linux {
        return VmFacts {
            architecture: Check::Unsupported,
            distribution: Check::Unsupported,
            kernel: Check::Unsupported,
            cpu_virtualization: Check::Unsupported,
            user_namespaces: Check::Unsupported,
            kvm: Check::Unsupported,
            qemu: Check::Unsupported,
            qemu_launcher_containment: Check::Unsupported,
            resources,
            nested_virtualization: Check::Unsupported,
        };
    }
    let architecture = match observed_architecture {
        ImageArchitectureV1::Amd64 => Check::Ready(()),
        ImageArchitectureV1::Unknown => Check::Unknown,
        ImageArchitectureV1::Arm64 | ImageArchitectureV1::Unsupported => Check::Unsupported,
    };
    let distribution = host_distribution_check(evidence, generation);
    let kernel = host_evidence_check(
        evidence,
        generation,
        crate::app::setup_attention::catalog::PrerequisiteCapability::LinuxKernel,
    );
    let user_namespaces = host_user_namespaces_check(evidence, generation);
    let cpu_virtualization = match probe.cpu_virtualization_flags() {
        Ok(Some(true)) => Check::Ready(()),
        Ok(Some(false)) => Check::Missing,
        Ok(None) | Err(_) => Check::Unknown,
    };
    let nested_virtualization = match probe.virtualization_present() {
        Ok(Some(true)) => Check::Unsupported,
        Ok(Some(false)) => Check::Ready(()),
        Ok(None) | Err(_) => Check::Unknown,
    };
    let kvm = if architecture == Check::Unsupported {
        Check::Unsupported
    } else {
        match nested_virtualization {
            Check::Unsupported => Check::Unsupported,
            Check::Ready(()) => match probe.open_kvm() {
                Ok(_) => Check::Ready(()),
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => Check::Missing,
                Err(error) if error.kind() == std::io::ErrorKind::PermissionDenied => {
                    Check::PermissionDenied
                }
                Err(_) => Check::Unknown,
            },
            _ => Check::Unknown,
        }
    };
    if architecture == Check::Unsupported {
        return VmFacts {
            architecture,
            distribution,
            kernel,
            cpu_virtualization,
            user_namespaces,
            kvm,
            qemu: Check::Unsupported,
            qemu_launcher_containment: Check::Unsupported,
            resources,
            nested_virtualization,
        };
    }
    let launcher_containment = match probe.launcher_containment() {
        Ok(()) => Check::Ready(()),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Check::Missing,
        Err(error) if error.kind() == std::io::ErrorKind::PermissionDenied => {
            Check::PermissionDenied
        }
        Err(error) if error.kind() == std::io::ErrorKind::Unsupported => Check::Unsupported,
        Err(_) => Check::Unknown,
    };
    let qemu_image_tool = match probe.qemu_image_tool() {
        Ok(()) => Check::Ready(()),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Check::Missing,
        Err(error) if error.kind() == std::io::ErrorKind::PermissionDenied => {
            Check::PermissionDenied
        }
        Err(_) => Check::Unknown,
    };
    let (version, accelerators) = if launcher_containment == Check::Ready(()) {
        (
            probe.qemu_output(&["--version"]),
            probe.qemu_output(&["-accel", "help"]),
        )
    } else {
        let error_kind = match launcher_containment {
            Check::Missing => std::io::ErrorKind::NotFound,
            Check::PermissionDenied => std::io::ErrorKind::PermissionDenied,
            Check::Unsupported => std::io::ErrorKind::Unsupported,
            Check::Ready(()) | Check::Unknown => std::io::ErrorKind::Other,
        };
        let blocked = || {
            Err(std::io::Error::new(
                error_kind,
                "QEMU launcher trust check failed",
            ))
        };
        (blocked(), blocked())
    };
    let qemu = match (version, accelerators) {
        (Err(error), _) if error.kind() == std::io::ErrorKind::NotFound => Check::Missing,
        (_, Err(error)) if error.kind() == std::io::ErrorKind::NotFound => Check::Missing,
        (Err(error), _) | (_, Err(error))
            if error.kind() == std::io::ErrorKind::PermissionDenied =>
        {
            Check::PermissionDenied
        }
        (Ok(Some(output)), Ok(Some(accelerators))) => {
            let version = output
                .split(|byte| *byte == b'\n')
                .next()
                .and_then(|line| std::str::from_utf8(line).ok())
                .map(|line| line.strip_suffix('\r').unwrap_or(line).to_owned());
            let kvm_acceleration = std::str::from_utf8(&accelerators)
                .ok()
                .map(|output| output.lines().any(|line| line.trim() == "kvm"));
            match (
                version.filter(|version| version_is_valid(version)),
                kvm_acceleration,
            ) {
                (Some(version), Some(kvm_acceleration)) => Check::Ready(QemuCapabilities {
                    version,
                    kvm_acceleration,
                    qemu_image_tool,
                }),
                _ => Check::Unknown,
            }
        }
        _ => Check::Unknown,
    };
    VmFacts {
        architecture,
        distribution,
        kernel,
        cpu_virtualization,
        nested_virtualization,
        user_namespaces,
        kvm,
        qemu,
        qemu_launcher_containment: launcher_containment,
        resources,
    }
}

fn version_is_valid(version: &str) -> bool {
    if version.len() > 160 || version.chars().any(char::is_control) {
        return false;
    }
    let Some(version) = version.strip_prefix("QEMU emulator version ") else {
        return false;
    };
    let version = version.split_ascii_whitespace().next().unwrap_or_default();
    let mut parts = version.split('.');
    let numeric = [parts.next(), parts.next(), parts.next()];
    numeric.into_iter().all(|part| {
        part.is_some_and(|part| !part.is_empty() && part.bytes().all(|byte| byte.is_ascii_digit()))
    }) && parts.next().is_none()
}
