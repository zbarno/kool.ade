use crate::app::{
    bootstrap_remediation::{
        HostIdentity, OperationRegistry, ProductionOperationRegistry, RemediationPlan,
        RemediationRequest, TrustedInvocation, TrustedProgram, plan::PlanConsent,
        qemu_package_arguments,
    },
    bootstrap_repair_policy::{Host, Operation, Proposal},
    setup_attention::catalog::LinuxRootlessRecipeV1 as Recipe,
};

fn qemu_plan(os_version: &str) -> RemediationPlan {
    let identity = HostIdentity::from_facts(
        Host::Linux,
        os_version,
        "synthetic-machine",
        "synthetic-user",
    )
    .unwrap();
    let proposal = Proposal {
        schema_version: 1,
        operation: Operation::InstallLinuxQemu,
        host: Host::Linux,
        prerequisite_id: "setup:computer:qemu".into(),
        image_profile: None,
    };
    let request = RemediationRequest::from_proposal_for_test(&proposal, &identity).unwrap();
    RemediationPlan::build_for_test(&request, &identity).unwrap()
}

#[test]
fn qemu_plan_is_consent_bound_and_compiles_to_the_fixed_internal_runner() {
    for os_version in [
        "debian-13-amd64-kernel-6.12.1",
        "ubuntu-24.04-amd64-kernel-6.8.0",
        "pop-24.04-amd64-kernel-6.17.9",
    ] {
        let plan = qemu_plan(os_version);
        assert_eq!(plan.operation, Operation::InstallLinuxQemu);
        assert_eq!(plan.prerequisite_id, "setup:computer:qemu");
        assert_eq!(
            plan.privilege,
            crate::app::bootstrap_remediation::PrivilegeLevel::Administrator
        );
        assert!(plan.requires_consent);
        assert_eq!(
            plan.required_consents,
            [PlanConsent::QemuPackageInstallation]
        );
        assert!(plan.file_changes.is_empty());
        assert!(plan.service_changes.is_empty());
        assert_eq!(plan.verification_probe_ids, ["host:linux:qemu"]);

        let invocation = ProductionOperationRegistry.compile(&plan).unwrap();
        assert_eq!(invocation.program(), TrustedProgram::LinuxQemuPackageSetup);
    }
}

#[test]
fn qemu_apt_arguments_install_only_the_fixed_package_pair_without_upgrade_or_removal() {
    let invocation = TrustedInvocation::new(
        Host::Linux,
        TrustedProgram::AptGet,
        qemu_package_arguments(Recipe::Ubuntu2404Amd64).unwrap(),
        std::time::Duration::from_secs(120),
    )
    .unwrap();
    let arguments = invocation.arguments_for_test();
    assert_eq!(
        &arguments[arguments.len() - 2..],
        ["qemu-system-x86", "qemu-utils"]
    );
    assert!(arguments.contains(&"--no-upgrade".into()));
    assert!(arguments.contains(&"--no-remove".into()));
    assert!(arguments.contains(&"--no-install-recommends".into()));
    assert!(!arguments.iter().any(|arg| arg.starts_with("docker")));
}
