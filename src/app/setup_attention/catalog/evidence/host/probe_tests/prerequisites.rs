use crate::app::setup_attention::catalog::{
    EvidenceCode as Code, EvidenceState as State, LinuxPackageManagerV1 as PackageManager,
    ProbeObservationV1 as Observation,
};

use super::super::{EntryKind, ProbeRead};
use super::fixture::{FixtureSource, UID, inspect};

#[test]
fn user_namespace_probe_separates_disabled_inaccessible_and_malformed() {
    let mut disabled = FixtureSource::ready();
    disabled.text("/proc/sys/user/max_user_namespaces", "0\n");
    assert_eq!(inspect(&disabled).user_namespaces.state, State::Unsupported);

    let mut denied = FixtureSource::ready();
    denied.text.insert(
        "/proc/sys/user/max_user_namespaces".into(),
        ProbeRead::Inaccessible,
    );
    let denied = inspect(&denied).user_namespaces;
    assert_eq!(denied.state, State::Unknown);
    assert_eq!(denied.code, Code::LinuxProbeInaccessible);

    let mut malformed = FixtureSource::ready();
    malformed.text("/proc/sys/user/max_user_namespaces", "many\n");
    assert_eq!(inspect(&malformed).user_namespaces.state, State::Failed);

    let mut restricted = FixtureSource::ready();
    restricted.text(
        "/proc/sys/kernel/apparmor_restrict_unprivileged_userns",
        "1\n",
    );
    assert_eq!(inspect(&restricted).user_namespaces.state, State::Unknown);
}

#[test]
fn subordinate_id_checks_separate_missing_denied_malformed_and_short_ranges() {
    let mut missing = FixtureSource::ready();
    missing.text.remove("/etc/subuid");
    let missing = inspect(&missing);
    assert_eq!(missing.subuid.state, State::Unknown);
    assert_eq!(missing.subuid.code, Code::HostCapabilityNotProbed);
    assert_eq!(missing.uid_mappings.state, State::Unknown);

    let mut denied = FixtureSource::ready();
    denied
        .text
        .insert("/etc/subgid".into(), ProbeRead::Inaccessible);
    assert_eq!(inspect(&denied).subgid.state, State::Unknown);

    let mut malformed = FixtureSource::ready();
    malformed.text("/etc/subuid", "developer:overflow:65536\n");
    assert_eq!(inspect(&malformed).subuid.state, State::Failed);

    let mut short = FixtureSource::ready();
    short.text("/etc/subgid", "developer:200000:1024\n");
    let short = inspect(&short);
    assert_eq!(short.subgid.state, State::Unknown);
    assert_eq!(short.subgid.code, Code::HostCapabilityNotProbed);
    assert_eq!(short.uid_mappings.state, State::Unknown);
}

#[test]
fn cgroup_probe_requires_v2_and_delegated_cpu_memory_and_pid_controllers() {
    let mut v1 = FixtureSource::ready();
    v1.text.remove("/sys/fs/cgroup/cgroup.controllers");
    assert_eq!(inspect(&v1).cgroups.state, State::Unsupported);

    let mut no_delegation = FixtureSource::ready();
    no_delegation.text(
        "/sys/fs/cgroup/user.slice/user-1000.slice/user@1000.service/cgroup.subtree_control",
        "memory pids\n",
    );
    assert_eq!(inspect(&no_delegation).cgroups.state, State::Missing);

    let mut controllers_unavailable = FixtureSource::ready();
    controllers_unavailable.text(
        "/sys/fs/cgroup/user.slice/user-1000.slice/user@1000.service/cgroup.controllers",
        "\n",
    );
    assert_eq!(
        inspect(&controllers_unavailable).cgroups.state,
        State::Missing
    );

    let mut denied = FixtureSource::ready();
    denied.text.insert(
        "/sys/fs/cgroup/cgroup.controllers".into(),
        ProbeRead::Inaccessible,
    );
    assert_eq!(inspect(&denied).cgroups.state, State::Unknown);

    let mut malformed = FixtureSource::ready();
    malformed.text("/sys/fs/cgroup/cgroup.controllers", "cpu ??? pids\n");
    assert_eq!(inspect(&malformed).cgroups.state, State::Failed);
}

#[test]
fn systemd_probe_requires_current_user_owned_runtime_and_sockets() {
    let mut wrong_owner = FixtureSource::ready();
    wrong_owner.set_metadata(
        "/run/user/1000/systemd/private",
        EntryKind::Socket,
        0,
        0o600,
        false,
    );
    assert_eq!(inspect(&wrong_owner).service_manager.state, State::Failed);

    let mut insecure_runtime = FixtureSource::ready();
    insecure_runtime.set_metadata("/run/user/1000", EntryKind::Directory, UID, 0o777, false);
    assert_eq!(
        inspect(&insecure_runtime).service_manager.state,
        State::Failed
    );

    let mut missing_bus = FixtureSource::ready();
    missing_bus.metadata.remove("/run/user/1000/bus");
    assert_eq!(inspect(&missing_bus).service_manager.state, State::Missing);

    let mut denied = FixtureSource::ready();
    denied.metadata.insert(
        "/run/user/1000/systemd/private".into(),
        ProbeRead::Inaccessible,
    );
    assert_eq!(inspect(&denied).service_manager.state, State::Unknown);
}

#[test]
fn package_manager_detection_is_matched_to_the_os_release_id() {
    let mut wrong_manager = FixtureSource::ready();
    wrong_manager.set_executable("apt-get", false);
    wrong_manager.set_executable("dnf", true);
    let facts = inspect(&wrong_manager);
    assert_eq!(facts.package_manager.state, State::Missing);
    assert!(matches!(
        facts.package_manager.observation,
        Some(Observation::LinuxPackageManager {
            expected: Some(PackageManager::AptGet),
            found: None,
        })
    ));

    let mut fedora = FixtureSource::ready();
    fedora.text("/etc/os-release", "ID=fedora\nVERSION_ID=42\n");
    fedora.set_executable("apt-get", false);
    fedora.set_executable("dnf", true);
    let facts = inspect(&fedora);
    assert_eq!(facts.package_manager.state, State::Ready);
    assert!(matches!(
        facts.package_manager.observation,
        Some(Observation::LinuxPackageManager {
            expected: Some(PackageManager::Dnf),
            found: Some(PackageManager::Dnf),
        })
    ));

    let mut unknown_distro = FixtureSource::ready();
    unknown_distro.text(
        "/etc/os-release",
        "ID=synthetic-distro\nID_LIKE=ubuntu\nVERSION_ID=1\n",
    );
    assert_eq!(
        inspect(&unknown_distro).package_manager.state,
        State::Unknown
    );
}

#[test]
fn fedora_45_recipe_requires_the_pinned_dnf5_executable() {
    let mut fedora = FixtureSource::ready();
    fedora.text("/etc/os-release", "ID=fedora\nVERSION_ID=45\n");
    fedora.set_executable("dnf", true);
    fedora.set_executable("dnf5", false);
    assert_eq!(inspect(&fedora).package_manager.state, State::Missing);

    fedora.set_executable("dnf5", true);
    let facts = inspect(&fedora);
    assert_eq!(facts.package_manager.state, State::Ready);
    assert!(matches!(
        facts.package_manager.observation,
        Some(Observation::LinuxPackageManager {
            expected: Some(PackageManager::Dnf),
            found: Some(PackageManager::Dnf),
        })
    ));
}
