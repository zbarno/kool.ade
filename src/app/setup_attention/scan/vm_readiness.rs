#[cfg(test)]
use crate::app::setup_attention::catalog::Finding;
#[cfg(test)]
use crate::app::setup_attention::catalog::FindingStatus;
use crate::app::setup_attention::catalog::{HostKind, ImageArchitectureV1};

mod evidence;
mod facts;
mod findings;
mod guest_image;
mod probe;
mod resources;

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
    distribution: Check<()>,
    kernel: Check<()>,
    cpu_virtualization: Check<()>,
    user_namespaces: Check<()>,
    kvm: Check<()>,
    qemu: Check<QemuCapabilities>,
    qemu_launcher_containment: Check<()>,
    resources: resources::ResourceCapacity,
    nested_virtualization: Check<()>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct QemuCapabilities {
    version: String,
    kvm_acceleration: bool,
    qemu_image_tool: Check<()>,
}

trait VmHostProbe {
    fn cpu_virtualization_flags(&self) -> std::io::Result<Option<bool>>;
    fn virtualization_present(&self) -> std::io::Result<Option<bool>>;
    fn open_kvm(&self) -> std::io::Result<()>;
    fn qemu_output(&self, arguments: &[&str]) -> std::io::Result<Option<Vec<u8>>>;
    fn qemu_image_tool(&self) -> std::io::Result<()>;
    fn launcher_containment(&self) -> std::io::Result<()>;
}

pub(super) fn append_findings(
    snapshot: &mut crate::app::setup_attention::catalog::MachineSnapshotV1,
    host: HostKind,
    architecture: ImageArchitectureV1,
    evidence: &[crate::app::setup_attention::catalog::ProbeEvidenceV1],
    generation: u64,
) -> (bool, bool) {
    let capacity = resources::probe();
    let facts = facts::collect(host, architecture, evidence, generation, capacity);
    let image_probe = guest_image::probe();
    let kvm_opened = facts.architecture == Check::Ready(())
        && facts.kernel == Check::Ready(())
        && facts.user_namespaces == Check::Ready(())
        && facts.cpu_virtualization == Check::Ready(())
        && facts.nested_virtualization == Check::Ready(())
        && facts.kvm == Check::Ready(());
    if host == HostKind::Linux {
        evidence::append(snapshot, &facts, image_probe);
    }
    snapshot
        .findings
        .extend(findings::build_with_guest_image_state(
            facts,
            image_probe.state,
        ));
    snapshot
        .findings
        .sort_by(|left, right| left.id.cmp(&right.id));
    snapshot
        .findings
        .dedup_by(|left, right| left.id == right.id);
    (
        kvm_opened,
        guest_image::ready_for_host(host, architecture, image_probe.state),
    )
}

pub(super) fn append_checked_domains(
    domains: &mut Vec<String>,
    kvm_opened: bool,
    guest_image_ready: bool,
) {
    if kvm_opened {
        domains.push("computer:kvm".into());
        domains.sort();
        domains.dedup();
    }
    if guest_image_ready {
        domains.push("computer:guest-image".into());
        domains.sort();
        domains.dedup();
    }
}

#[cfg(test)]
fn findings_for(facts: VmFacts) -> Vec<Finding> {
    findings::build_with_guest_image_state(facts, guest_image::probe().state)
}

#[cfg(test)]
#[path = "vm_readiness/tests.rs"]
mod tests;
