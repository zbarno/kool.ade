use crate::app::setup_attention::catalog::{Finding, FindingStatus, HostKind, ImageArchitectureV1};

mod probe;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum Check<T> {
    Ready(T),
    Missing,
    PermissionDenied,
    Unknown,
    Unsupported,
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct VmFacts {
    architecture: Check<()>,
    kvm: Check<()>,
    qemu: Check<QemuCapabilities>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct QemuCapabilities {
    kvm_acceleration: bool,
}

fn findings(facts: VmFacts) -> Vec<Finding> {
    let mut result = Vec::new();
    let (image_status, image_explanation, image_next) = match facts.architecture {
        Check::Ready(()) => (
            FindingStatus::Unverified,
            "The approved guest image catalog and its integrity have not been connected to this setup scan yet.",
            "Rescan after the signed x86_64 guest image catalog is available.",
        ),
        Check::Missing | Check::Unknown | Check::PermissionDenied => (
            FindingStatus::Unverified,
            "Host architecture could not be verified for the disposable computer image.",
            "Verify the host architecture and rescan.",
        ),
        Check::Unsupported => (
            FindingStatus::Unsupported,
            "Disposable computer images are currently supported only on x86_64 Linux without nested virtualization.",
            "Use a supported x86_64 Linux host. macOS, Windows, ARM, and nested virtualization are not yet supported.",
        ),
    };
    result.push(task(
        "setup:computer:image:amd64",
        "Verify approved computer image",
        image_explanation,
        image_next,
        image_status,
        vec![],
    ));
    if facts.kvm != Check::Ready(()) {
        let (status, detail) = match facts.kvm {
            Check::Missing => (
                FindingStatus::NeedsAttention,
                "The KVM device is not available on this host.",
            ),
            Check::PermissionDenied => (
                FindingStatus::NeedsAttention,
                "The KVM device exists, but this user cannot open it.",
            ),
            Check::Unknown => (
                FindingStatus::Unverified,
                "KVM availability could not be verified safely.",
            ),
            Check::Unsupported => (
                FindingStatus::Unsupported,
                "KVM is supported only on verified x86_64 Linux hosts without nested virtualization.",
            ),
            Check::Ready(()) => unreachable!(),
        };
        result.push(task(
            "setup:computer:kvm", "Enable KVM for disposable computers", detail,
            "Check firmware virtualization and the operating system's KVM device access. Rescan after any approved system change.",
            status, vec![],
        ));
    }
    let (status, detail) = match &facts.qemu {
        Check::Missing => (
            FindingStatus::NeedsAttention,
            "A supported QEMU system emulator was not found.",
        ),
        Check::Unknown => (
            FindingStatus::Unverified,
            "QEMU version and acceleration support could not be verified.",
        ),
        Check::PermissionDenied => (
            FindingStatus::Unverified,
            "The QEMU executable could not be read and verified.",
        ),
        Check::Unsupported => (
            FindingStatus::Unsupported,
            "This host is not a supported QEMU platform.",
        ),
        Check::Ready(qemu) if !qemu.kvm_acceleration => (
            FindingStatus::NeedsAttention,
            "QEMU is installed, but this build does not report KVM acceleration support.",
        ),
        Check::Ready(_) => (
            FindingStatus::Unverified,
            "QEMU reports KVM support, but a minimum supported version and real VM startup have not been validated.",
        ),
    };
    result.push(task(
            "setup:computer:qemu", "Verify QEMU support", detail,
            "Use the operating system's supported package source, then rescan. Kool.ad/e does not run installation commands automatically.",
            status, vec!["setup:computer:kvm".into()],
    ));
    for (id, title, why, next, prerequisites) in [
        (
            "setup:computer:storage",
            "Prepare isolated computer storage",
            "Encrypted per-session overlays and reserved disk capacity have not been verified.",
            "Storage readiness will be available after the approved VM storage backend is installed.",
            vec!["setup:computer:image:amd64".into()],
        ),
        (
            "setup:computer:viewer",
            "Prepare the interactive viewer",
            "The isolated graphical display transport is not available yet.",
            "Viewer readiness will be checked when the approved graphical transport is installed.",
            vec!["setup:computer:image:amd64".into()],
        ),
        (
            "setup:computer:personal-encryption",
            "Prepare personal computer encryption",
            "A personal vault and its key service have not been verified.",
            "Configure the approved encrypted vault and key service before using personal data in a computer session.",
            vec!["setup:computer:storage".into()],
        ),
    ] {
        result.push(task(
            id,
            title,
            why,
            next,
            FindingStatus::Unverified,
            prerequisites,
        ));
    }
    result.sort_by(|left, right| left.id.cmp(&right.id));
    result
}

fn task(
    id: &str,
    title: &str,
    explanation: &str,
    next_step: &str,
    status: FindingStatus,
    prerequisites: Vec<String>,
) -> Finding {
    Finding {
        id: id.into(),
        title: title.into(),
        explanation: explanation.into(),
        next_step: next_step.into(),
        status,
        prerequisites,
    }
}

fn collect(host: HostKind, observed_architecture: ImageArchitectureV1) -> VmFacts {
    if host != HostKind::Linux {
        return VmFacts {
            architecture: Check::Unsupported,
            kvm: Check::Unsupported,
            qemu: Check::Unsupported,
        };
    }
    let architecture = match observed_architecture {
        ImageArchitectureV1::Amd64 => Check::Ready(()),
        ImageArchitectureV1::Unknown => Check::Unknown,
        ImageArchitectureV1::Arm64 | ImageArchitectureV1::Unsupported => Check::Unsupported,
    };
    let kvm = match nested_virtualization() {
        Some(true) => Check::Unsupported,
        None => Check::Unknown,
        Some(false) => match std::fs::OpenOptions::new()
            .read(true)
            .write(true)
            .open("/dev/kvm")
        {
            Ok(_) => Check::Ready(()),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => Check::Missing,
            Err(error) if error.kind() == std::io::ErrorKind::PermissionDenied => {
                Check::PermissionDenied
            }
            Err(_) => Check::Unknown,
        },
    };
    let version = probe::qemu_output(&["--version"]);
    let accelerators = probe::qemu_output(&["-accel", "help"]);
    let qemu = match (version, accelerators) {
        (Err(error), _) if error.kind() == std::io::ErrorKind::NotFound => Check::Missing,
        (_, Err(error)) if error.kind() == std::io::ErrorKind::NotFound => Check::Missing,
        (Err(error), _) | (_, Err(error))
            if error.kind() == std::io::ErrorKind::PermissionDenied =>
        {
            Check::PermissionDenied
        }
        (Ok(Some(output)), Ok(Some(accelerators))) => {
            let version = String::from_utf8_lossy(&output)
                .lines()
                .next()
                .unwrap_or_default()
                .to_owned();
            let kvm_acceleration = String::from_utf8_lossy(&accelerators)
                .lines()
                .any(|line| line.trim() == "kvm");
            if version.is_empty() || version.len() > 160 || version.chars().any(char::is_control) {
                Check::Unknown
            } else {
                Check::Ready(QemuCapabilities { kvm_acceleration })
            }
        }
        _ => Check::Unknown,
    };
    VmFacts {
        architecture,
        kvm,
        qemu,
    }
}

fn nested_virtualization() -> Option<bool> {
    match std::fs::read_to_string("/sys/hypervisor/type") {
        Ok(value) => Some(!value.trim().is_empty()),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Some(false),
        Err(_) => None,
    }
}

pub(super) fn append_findings(
    findings: &mut Vec<Finding>,
    host: HostKind,
    architecture: ImageArchitectureV1,
) -> bool {
    let facts = collect(host, architecture);
    let kvm_opened = facts.kvm == Check::Ready(());
    findings.extend(findings_for(facts));
    findings.sort_by(|left, right| left.id.cmp(&right.id));
    findings.dedup_by(|left, right| left.id == right.id);
    kvm_opened
}

pub(super) fn append_checked_domains(domains: &mut Vec<String>, kvm_opened: bool) {
    if kvm_opened {
        domains.push("computer:kvm".into());
        domains.sort();
        domains.dedup();
    }
}

fn findings_for(facts: VmFacts) -> Vec<Finding> {
    findings(facts)
}

#[cfg(test)]
#[path = "vm_readiness/tests.rs"]
mod tests;
