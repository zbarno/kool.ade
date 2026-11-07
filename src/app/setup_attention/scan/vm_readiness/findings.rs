use super::{Check, VmFacts, facts};
use crate::app::setup_attention::catalog::{Finding, FindingStatus};

pub(super) fn build_with_guest_image_state(
    facts: VmFacts,
    guest_image_state: crate::app::setup_attention::catalog::EvidenceState,
) -> Vec<Finding> {
    let mut result = Vec::new();
    let image_finding = match facts.architecture {
        Check::Ready(())
            if guest_image_state == crate::app::setup_attention::catalog::EvidenceState::Ready =>
        {
            None
        }
        Check::Ready(())
            if facts.distribution == Check::Unsupported
                || facts.nested_virtualization == Check::Unsupported =>
        {
            Some((
                FindingStatus::Unsupported,
                "Disposable computer images require a supported x86_64 Linux host without nested virtualization.",
                "Use a supported x86_64 Linux host without nested virtualization.",
            ))
        }
        Check::Ready(()) => match guest_image_state {
            crate::app::setup_attention::catalog::EvidenceState::Ready => None,
            crate::app::setup_attention::catalog::EvidenceState::Missing => Some((
                FindingStatus::Unsupported,
                "This build has no promoted, pinned guest image catalog.",
                "Keep sessions off. Use an application build that includes an approved guest image catalog, then rescan.",
            )),
            _ => Some((
                FindingStatus::Unverified,
                "The signed guest image catalog, anti-rollback state, or cached image could not be verified.",
                "Keep sessions blocked. Restore only app-owned catalog/cache state, then rescan; do not import an unverified image.",
            )),
        },
        Check::Missing | Check::Unknown | Check::PermissionDenied => Some((
            FindingStatus::Unverified,
            "Host architecture could not be verified for the disposable computer image.",
            "Verify the host architecture and rescan.",
        )),
        Check::Unsupported => Some((
            FindingStatus::Unsupported,
            "Disposable computer images are currently supported only on x86_64 Linux without nested virtualization.",
            "Use a supported x86_64 Linux host. macOS, Windows, ARM, and nested virtualization are not yet supported.",
        )),
    };
    if let Some((status, explanation, next_step)) = image_finding {
        result.push(task(
            "setup:computer:image:amd64",
            "Verify approved computer image",
            explanation,
            next_step,
            status,
            vec![],
        ));
    }
    let (ram_short, disk_short) = facts.resources.shortages();
    let (storage_status, storage_explanation, storage_next) = if ram_short || disk_short {
        let shortage = match (ram_short, disk_short) {
            (true, true) => {
                "Available memory and storage are below the minimum disposable guest budget."
            }
            (true, false) => "Available memory is below the minimum disposable guest budget.",
            (false, true) => "Available storage is below the minimum disposable guest budget.",
            _ => unreachable!(),
        };
        (
            FindingStatus::NeedsAttention,
            shortage,
            "Free the required host resources, then rescan. Kool.ad/e made no storage or memory changes.",
        )
    } else {
        let detail = match (
            facts.resources.available_storage_bytes.is_some(),
            facts.resources.filesystem_family.is_some(),
        ) {
            (true, true) => {
                "Free storage and its filesystem family were measured, but the signed image size and private per-session overlay capacity are not verified."
            }
            (true, false) => {
                "Free storage was measured, but the filesystem family could not be classified and overlay capacity remains unverified."
            }
            (false, _) => {
                "Free storage and filesystem family could not both be measured; overlay capacity remains unverified."
            }
        };
        (
            FindingStatus::Unverified,
            detail,
            "Keep sessions off until Kool.ad/e verifies the promoted signed image, exact overlay capacity, and private per-session storage, then rescan.",
        )
    };
    result.push(task(
        "setup:computer:storage",
        "Prepare isolated computer storage",
        storage_explanation,
        storage_next,
        storage_status,
        vec![
            "setup:computer:image:amd64".into(),
            "setup:computer:qemu".into(),
        ],
    ));
    let kvm_task_needed = facts.architecture != Check::Ready(())
        || facts.cpu_virtualization != Check::Ready(())
        || facts.nested_virtualization != Check::Ready(())
        || facts.kvm != Check::Ready(())
        || facts.kernel != Check::Ready(())
        || facts.user_namespaces != Check::Ready(());
    if kvm_task_needed {
        let (status, detail, next_step) = facts::kvm_finding(&facts);
        result.push(task(
            "setup:computer:kvm",
            "Enable KVM for disposable computers",
            detail,
            next_step,
            status,
            vec![],
        ));
    }
    let mut vm_prerequisites = vec![
        "setup:computer:image:amd64".to_owned(),
        "setup:computer:qemu".to_owned(),
        "setup:computer:storage".to_owned(),
    ];
    if kvm_task_needed {
        vm_prerequisites.push("setup:computer:kvm".into());
    }
    vm_prerequisites.sort();
    let (status, detail, next_step) = if facts.qemu_launcher_containment != Check::Ready(()) {
        match facts.qemu_launcher_containment {
            Check::Missing => (
                FindingStatus::NeedsAttention,
                "The fixed QEMU launcher executable is missing.",
                "Use Fix for me to review the supported distro QEMU packages, then rescan.",
            ),
            Check::PermissionDenied => (
                FindingStatus::NeedsAttention,
                "The fixed QEMU launcher is not protected by trusted system ownership and permissions.",
                "Ask an administrator to repair the distro QEMU package. Do not change executable permissions.",
            ),
            Check::Unknown => (
                FindingStatus::Unverified,
                "QEMU launcher ownership and containment could not be verified.",
                "Keep computer sessions blocked until the fixed system QEMU launcher can be verified, then rescan.",
            ),
            Check::Unsupported => (
                FindingStatus::Unsupported,
                "The QEMU launcher check is unsupported on this host.",
                "Use a supported x86_64 Linux host with the fixed native QEMU package.",
            ),
            Check::Ready(()) => unreachable!(),
        }
    } else {
        match &facts.qemu {
            Check::Missing if facts.distribution == Check::Unsupported => (
                FindingStatus::Unsupported,
                "QEMU is missing, and this Linux distribution has no verified native setup recipe.",
                "Use a supported x86_64 Linux distribution with an approved QEMU setup path. No package command is guessed.",
            ),
            Check::Missing => (
                FindingStatus::NeedsAttention,
                "A supported QEMU system emulator was not found.",
                if facts.distribution == Check::Ready(()) {
                    "Use Fix for me to review installation of the distro QEMU packages from verified official archives, then rescan. Computer sessions stay blocked until QEMU and KVM are verified."
                } else {
                    "Kool.ad/e cannot identify a verified QEMU setup path for this unknown distribution. Keep computer sessions blocked; no package command is guessed."
                },
            ),
            Check::Unknown => (
                FindingStatus::Unverified,
                "QEMU version and acceleration support could not be verified.",
                "Kool.ad/e could not verify QEMU. Keep sessions blocked until a supported QEMU installation is available and rescan.",
            ),
            Check::PermissionDenied => (
                FindingStatus::Unverified,
                "The QEMU executable could not be read and verified.",
                "Do not change executable permissions. Ask an administrator to repair the native QEMU package, then rescan.",
            ),
            Check::Unsupported => (
                FindingStatus::Unsupported,
                "This host is not a supported QEMU platform.",
                "Use a supported x86_64 Linux host. No VM setup changes were made.",
            ),
            Check::Ready(qemu) if qemu.qemu_image_tool != Check::Ready(()) => {
                match qemu.qemu_image_tool {
                    Check::Missing => (
                        FindingStatus::NeedsAttention,
                        "The QEMU image utility needed for disposable overlays was not found.",
                        "Use Fix for me to review installation of the distro QEMU packages from verified official archives, then rescan.",
                    ),
                    Check::PermissionDenied => (
                        FindingStatus::NeedsAttention,
                        "The QEMU image utility could not be verified as a trusted native package.",
                        "Ask an administrator to repair the distro QEMU package, then rescan. Do not change executable permissions.",
                    ),
                    Check::Unknown => (
                        FindingStatus::Unverified,
                        "The QEMU image utility could not be verified safely.",
                        "Keep sessions blocked until the fixed system QEMU image utility can be verified, then rescan.",
                    ),
                    Check::Unsupported => (
                        FindingStatus::Unsupported,
                        "The QEMU image utility is unsupported on this host.",
                        "Use a supported x86_64 Linux host with an approved QEMU package path.",
                    ),
                    Check::Ready(()) => unreachable!(),
                }
            }
            Check::Ready(qemu) if !qemu.kvm_acceleration => (
                FindingStatus::NeedsAttention,
                "QEMU is installed, but this build does not report KVM acceleration support.",
                "Review the distro QEMU package setup or repair the installed package through your administrator, then rescan. Computer sessions stay blocked until KVM acceleration is verified.",
            ),
            Check::Ready(_) => (
                FindingStatus::Unverified,
                "QEMU reports KVM support, but a minimum supported version and real VM startup have not been validated.",
                "Keep VM startup blocked until the integrated backend validates this QEMU version.",
            ),
        }
    };
    let detail = match &facts.qemu {
        Check::Ready(qemu) => format!("{detail} Detected version: {}.", qemu.version),
        _ => detail.to_owned(),
    };
    result.push(task(
        "setup:computer:qemu",
        "Verify QEMU support",
        &detail,
        next_step,
        status,
        vec![],
    ));
    for (id, title, why, next, status, prerequisites) in [
        (
            "setup:computer:network",
            "Prepare restricted internet access",
            "Disposable computer sessions stay offline until a verified, filtered gateway is available.",
            "Internet access is unsupported in this app release. Keep sessions offline until an approved filtered gateway and SSRF protections are implemented.",
            FindingStatus::Unsupported,
            vm_prerequisites.clone(),
        ),
        (
            "setup:computer:viewer",
            "Prepare the interactive viewer",
            "An authenticated, isolated graphical display transport is not available yet.",
            "Interactive viewing is unsupported in this app release. Keep the display transport disabled until an authenticated private viewer is implemented.",
            FindingStatus::Unsupported,
            vm_prerequisites.clone(),
        ),
        (
            "setup:computer:personal-encryption",
            "Prepare personal computer encryption",
            "A personal vault and its key service have not been verified.",
            "Personal encrypted sessions are unsupported in this app release. Keep personal data out of disposable computer sessions until a vault and key service are implemented.",
            FindingStatus::Unsupported,
            vm_prerequisites,
        ),
    ] {
        result.push(task(id, title, why, next, status, prerequisites));
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

#[cfg(test)]
#[path = "findings_tests.rs"]
mod tests;
