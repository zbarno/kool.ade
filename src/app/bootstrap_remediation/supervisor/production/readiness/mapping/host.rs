use crate::app::setup_attention::catalog as setup;

use super::Phase;

pub(in crate::app::bootstrap_remediation::supervisor::production::readiness) fn host_ready(
    id: &str,
    item: &setup::ProbeEvidenceV1,
    phase: Phase,
) -> bool {
    use setup::{
        EvidenceCode as C, EvidenceOrigin as O, EvidenceState as S, ProbeObservationV1 as V,
    };
    if item.origin != O::HostRuntime {
        return false;
    }
    match (id, item.state, item.code, &item.observation) {
        (
            "host:linux:package_manager",
            S::Ready,
            C::LinuxPackageManagerDetected,
            Some(V::LinuxPackageManager {
                expected: Some(setup::LinuxPackageManagerV1::AptGet),
                found: Some(setup::LinuxPackageManagerV1::AptGet),
            }),
        ) => true,
        (
            "host:linux:subuid" | "host:linux:subgid",
            S::Ready,
            C::LinuxHostCheckReady,
            Some(V::LinuxSubordinateIds { configured: true }),
        ) => true,
        (
            "host:linux:subuid" | "host:linux:subgid",
            S::Missing,
            C::LinuxHostCheckMissing,
            Some(V::LinuxSubordinateIds { configured: false }),
        ) => matches!(phase, Phase::Before),
        ("host:linux:uid_mappings", S::Ready, C::LinuxUidMappingsConfigured, None) => true,
        (
            "host:linux:rootless_recipe",
            S::Ready,
            C::LinuxRootlessRecipeCandidate,
            Some(V::LinuxRootlessRecipe {
                candidate: Some(setup::LinuxRootlessRecipeV1::Ubuntu2404Amd64),
                distribution_state: S::Ready,
            }),
        ) => true,
        (
            "host:linux:kernel",
            S::Ready,
            C::LinuxHostCheckReady,
            Some(V::LinuxKernel { version }),
        ) => version.major > 4 || (version.major == 4 && version.minor >= 15),
        (
            "host:linux:user_namespaces",
            S::Ready,
            C::LinuxHostCheckReady,
            Some(V::LinuxUserNamespaces {
                max_user_namespaces: Some(limit),
                unprivileged_clone_allowed,
                apparmor_userns_restricted,
            }),
        ) => {
            *limit > 0
                && *unprivileged_clone_allowed != Some(false)
                && *apparmor_userns_restricted != Some(true)
        }
        (
            "host:linux:cgroups",
            S::Ready,
            C::LinuxHostCheckReady,
            Some(V::LinuxCgroups {
                cgroup_v2: true,
                cpu_delegated: true,
                memory_delegated: true,
                pids_delegated: true,
            }),
        ) => true,
        (
            "host:linux:rootless_helpers",
            S::Ready,
            C::LinuxHostCheckReady,
            Some(V::LinuxRootlessHelpers {
                newuidmap: true,
                newgidmap: true,
                rootlesskit: true,
                dockerd_rootless: true,
                setuptool: true,
                ..
            }),
        ) => true,
        (
            "host:linux:service_manager",
            S::Ready,
            C::LinuxServiceManagerDetected,
            Some(V::LinuxServiceManager {
                runtime_directory_ready: true,
                private_socket_ready: true,
                bus_socket_ready: true,
            }),
        ) => true,
        (
            "host:linux:user_runtime_directory",
            S::Ready | S::Missing,
            C::LinuxServiceManagerDetected | C::LinuxHostCheckMissing,
            Some(V::LinuxServiceManager {
                runtime_directory_ready: true,
                ..
            }),
        ) => true,
        _ => false,
    }
}
