use super::*;
use crate::app::bootstrap_remediation::plan::PlanConsent;
use crate::app::bootstrap_remediation::{HostIdentity, RemediationRequest, TrustedInvocation};
use crate::app::bootstrap_repair_policy::Proposal;
use crate::app::setup_attention::catalog::LinuxRootlessRecipeV1 as Recipe;

fn rootless_plan(operation: Operation) -> RemediationPlan {
    rootless_plan_for_os(operation, "ubuntu-24.04-amd64-kernel-6.8.0")
}

fn rootless_plan_for_os(operation: Operation, os_version: &str) -> RemediationPlan {
    let prerequisite_id = match operation {
        Operation::InstallLinuxRootlessPackages => PACKAGE_OPERATION_ID,
        Operation::ConfigureLinuxSubordinateIds => SUBIDS_OPERATION_ID,
        Operation::InstallLinuxRootlessService => SERVICE_OPERATION_ID,
        _ => unreachable!(),
    };
    let identity =
        HostIdentity::from_facts(Host::Linux, os_version, "synthetic-machine", "1000").unwrap();
    let proposal = Proposal {
        schema_version: 1,
        operation,
        host: Host::Linux,
        prerequisite_id: prerequisite_id.into(),
        image_profile: None,
    };
    let request = RemediationRequest::from_proposal_for_test(&proposal, &identity).unwrap();
    RemediationPlan::build_for_test(&request, &identity).unwrap()
}

// Production boundary: verifies model-independent operation compilation,
// trusted argv construction, and package preflight binding. This test uses
// typed plans and synthetic identities; it needs no apt repository, Docker,
// systemd service, network, or privileged host changes.
#[test]
fn service_registry_compiles_only_the_internal_app_owned_operation() {
    let plan = rootless_plan(Operation::InstallLinuxRootlessService);
    let invocation = ProductionOperationRegistry.compile(&plan).unwrap();
    assert_eq!(
        invocation.program_for_test(),
        TrustedProgram::LinuxRootlessService
    );
    assert!(invocation.arguments_for_test().is_empty());
    assert!(invocation.uses_current_user_systemd_for_test());
}

#[test]
fn apt_invocation_has_fixed_packages_and_cannot_upgrade_remove_or_recommend() {
    let invocation = TrustedInvocation::new(
        Host::Linux,
        TrustedProgram::AptGet,
        apt::package_arguments(Recipe::Ubuntu2404Amd64).unwrap(),
        Duration::from_secs(120),
    )
    .unwrap();
    assert_eq!(invocation.program_for_test(), TrustedProgram::AptGet);
    assert_eq!(
        invocation.arguments_for_test(),
        [
            "--option=RootDir=/",
            "--option=Dir::Etc::sourcelist=/etc/apt/sources.list",
            "--option=Dir::Etc::sourceparts=/etc/apt/sources.list.d",
            "--option=Dir::State::lists=/var/lib/apt/lists",
            "--option=APT::Get::List-Cleanup=true",
            "--option=APT::Architecture=amd64",
            "--option=APT::Get::AllowUnauthenticated=false",
            "--option=Acquire::AllowInsecureRepositories=false",
            "--option=Acquire::AllowWeakRepositories=false",
            "--option=Acquire::AllowDowngradeToInsecureRepositories=false",
            "--option=Acquire::Check-Date=true",
            "--option=Acquire::Check-Valid-Until=true",
            "--option=Acquire::https::Verify-Peer=true",
            "--option=Acquire::https::Verify-Host=true",
            "install",
            "--assume-yes",
            "--no-upgrade",
            "--no-remove",
            "--no-install-recommends",
            "docker-ce",
            "docker-ce-cli",
            "containerd.io",
            "docker-ce-rootless-extras",
            "uidmap",
            "dbus-user-session",
        ]
    );
}

#[test]
fn package_registry_compiles_a_fixed_internal_bootstrap_operation() {
    let plan = rootless_plan(Operation::InstallLinuxRootlessPackages);
    let invocation = ProductionOperationRegistry.compile(&plan).unwrap();
    assert_eq!(
        invocation.program_for_test(),
        TrustedProgram::LinuxRootlessPackageSetup
    );
    assert_eq!(
        invocation.arguments_for_test(),
        ["recipe:ubuntu-24.04-amd64"]
    );
    assert!(invocation.uses_current_user_systemd_for_test());
    let preflight = invocation.native_elevation_preflight().unwrap();
    assert_eq!(preflight.program_for_test(), TrustedProgram::AptGet);
    assert_eq!(preflight.arguments_for_test(), ["--version"]);
    assert!(preflight.uses_current_user_systemd_for_test());
}

#[test]
fn fedora_package_registry_binds_the_exact_dnf_recipe_and_preflight() {
    let plan = rootless_plan_for_os(
        Operation::InstallLinuxRootlessPackages,
        "fedora-45-amd64-kernel-6.17.9",
    );
    let invocation = ProductionOperationRegistry.compile(&plan).unwrap();
    assert_eq!(
        invocation.program_for_test(),
        TrustedProgram::LinuxRootlessPackageSetup
    );
    assert_eq!(invocation.arguments_for_test(), ["recipe:fedora-45-amd64"]);
    let preflight = invocation.native_elevation_preflight().unwrap();
    assert_eq!(preflight.program_for_test(), TrustedProgram::Dnf);
    assert_eq!(
        preflight.arguments_for_test(),
        [
            "--setopt=cachedir=/var/cache/libdnf5",
            "--setopt=system_cachedir=/var/cache/libdnf5",
            "--version",
        ]
    );
}

#[test]
fn package_plan_requires_separate_rootful_service_activation_consent() {
    let plan = rootless_plan(Operation::InstallLinuxRootlessPackages);
    assert_eq!(
        plan.required_consents,
        [PlanConsent::RootfulDockerPackageActivation]
    );
    assert_eq!(plan.service_changes.len(), 1);
    assert!(
        PlanConsent::RootfulDockerPackageActivation
            .description()
            .contains("enable, or start")
    );
}

#[test]
fn fedora_package_plan_does_not_request_rootful_service_activation() {
    let plan = rootless_plan_for_os(
        Operation::InstallLinuxRootlessPackages,
        "fedora-45-amd64-kernel-6.17.9",
    );
    assert!(plan.required_consents.is_empty());
    assert!(plan.service_changes.is_empty());
}

#[test]
fn registry_revalidates_canonical_linux_task_and_recipe_before_compilation() {
    let registry = ProductionOperationRegistry;
    let mut plan = rootless_plan(Operation::InstallLinuxRootlessService);
    plan.prerequisite_id = "setup:linux:rootless:other".into();
    assert!(registry.compile(&plan).is_err());

    let mut plan = rootless_plan(Operation::InstallLinuxRootlessPackages);
    plan.host = Host::MacOs;
    assert!(registry.compile(&plan).is_err());

    let mut plan = rootless_plan(Operation::ConfigureLinuxSubordinateIds);
    plan.os_version = "ubuntu-24.04-arm64-kernel-6.8.0".into();
    assert!(registry.compile(&plan).is_err());
}

#[test]
fn elevation_preflight_is_fixed_version_only_and_preserves_execution_environment() {
    for (program, install_args) in [
        (
            TrustedProgram::AptGet,
            apt::package_arguments(Recipe::Ubuntu2404Amd64).unwrap(),
        ),
        (
            TrustedProgram::Usermod,
            subids::arguments_for(&subids::CurrentAccount {
                uid: 1000,
                name: "synthetic-user".into(),
            }),
        ),
    ] {
        let original = TrustedInvocation::new_for_current_user_systemd(
            Host::Linux,
            program,
            install_args,
            Duration::from_secs(30),
        )
        .unwrap();
        assert!(original.uses_current_user_systemd_for_test());
        let preflight = original.native_elevation_preflight().unwrap();
        assert_eq!(preflight.program_for_test(), program);
        assert_eq!(preflight.arguments_for_test(), ["--version"]);
        assert_eq!(
            preflight.uses_current_user_systemd_for_test(),
            original.uses_current_user_systemd_for_test()
        );
    }

    let not_elevated = TrustedInvocation::new(
        Host::Linux,
        TrustedProgram::Rootlesskit,
        vec![TrustedArgument::RootlesskitTrueExecutable],
        Duration::from_secs(10),
    )
    .unwrap();
    assert!(not_elevated.native_elevation_preflight().is_err());
}

#[test]
fn elevated_launch_gate_rejects_non_package_and_non_usermod_programs() {
    let invocation = TrustedInvocation::new(
        Host::Linux,
        TrustedProgram::Systemctl,
        vec![TrustedArgument::SystemctlUser],
        Duration::from_secs(10),
    )
    .unwrap();
    assert!(validate_before_elevated_launch(&invocation).is_err());
}

#[test]
fn current_user_subordinate_id_argument_is_bound_into_invocation_digest() {
    let make = |uid, name: &str| {
        TrustedInvocation::new(
            Host::Linux,
            TrustedProgram::Usermod,
            subids::arguments_for(&subids::CurrentAccount {
                uid,
                name: name.into(),
            }),
            Duration::from_secs(30),
        )
        .unwrap()
    };
    assert_ne!(
        make(1000, "synthetic-user")
            .binding_digest_for_test()
            .unwrap(),
        make(1001, "synthetic-other")
            .binding_digest_for_test()
            .unwrap()
    );
}
