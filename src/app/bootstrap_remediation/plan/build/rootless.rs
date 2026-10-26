use crate::app::setup_attention::catalog::LinuxRootlessRecipeV1;

use super::super::{
    ManagedFileChange, ManagedPackageChange, ManagedServiceChange, PlanConsent, PlanEffect,
    PrivilegeLevel, RemediationPlan, RollbackGuidance,
};

const SUPPORTED_HOST: &[&str] = &[
    "host:linux:rootless_recipe",
    "host:linux:kernel",
    "host:linux:user_namespaces",
    "host:linux:cgroups",
];

pub(super) fn install_packages(
    plan: &mut RemediationPlan,
    recipe: LinuxRootlessRecipeV1,
) -> anyhow::Result<()> {
    validate_recipe(recipe)?;
    let apt_recipe = matches!(
        recipe,
        LinuxRootlessRecipeV1::Debian13Amd64
            | LinuxRootlessRecipeV1::Ubuntu2404Amd64
            | LinuxRootlessRecipeV1::PopOs2404Amd64
    );
    if apt_recipe {
        plan.effects.push(PlanEffect::ConfigureDockerAptRepository);
        plan.file_changes.extend([
            ManagedFileChange::DockerAptKeyringDirectory,
            ManagedFileChange::DockerAptSigningKey,
            ManagedFileChange::DockerAptSourcesEntry,
        ]);
        plan.service_changes
            .push(ManagedServiceChange::SystemRootfulDockerServicePackageSideEffect);
        plan.required_consents
            .push(PlanConsent::RootfulDockerPackageActivation);
    } else {
        plan.effects.push(PlanEffect::ConfigureDockerDnfRepository);
        plan.file_changes.extend([
            ManagedFileChange::DockerDnfSigningKey,
            ManagedFileChange::DockerDnfRepositoryEntry,
        ]);
    }
    plan.effects.push(PlanEffect::InstallLinuxRootlessPackages);
    plan.package_changes.push(match recipe {
        LinuxRootlessRecipeV1::Debian13Amd64 => ManagedPackageChange::Debian,
        LinuxRootlessRecipeV1::Ubuntu2404Amd64 => ManagedPackageChange::Ubuntu,
        LinuxRootlessRecipeV1::PopOs2404Amd64 => ManagedPackageChange::PopOs,
        LinuxRootlessRecipeV1::Fedora45Amd64 => ManagedPackageChange::Fedora,
    });
    plan.precondition_probe_ids = host_preconditions(&["host:linux:package_manager"]);
    plan.verification_probe_ids = vec!["host:linux:rootless_helpers".into()];
    plan.privilege = PrivilegeLevel::Administrator;
    plan.rollback = RollbackGuidance::KeepInstalledPackagesForManualRecovery;
    plan.requires_consent = true;
    Ok(())
}

pub(super) fn configure_subordinate_ids(
    plan: &mut RemediationPlan,
    recipe: LinuxRootlessRecipeV1,
) -> anyhow::Result<()> {
    validate_recipe(recipe)?;
    plan.effects.push(PlanEffect::ConfigureLinuxSubordinateIds);
    plan.file_changes
        .push(ManagedFileChange::LinuxSubordinateIdMappings);
    plan.precondition_probe_ids = host_preconditions(&["host:linux:subuid", "host:linux:subgid"]);
    plan.verification_probe_ids = vec![
        "host:linux:subuid".into(),
        "host:linux:subgid".into(),
        "host:linux:uid_mappings".into(),
    ];
    plan.privilege = PrivilegeLevel::Administrator;
    plan.rollback = RollbackGuidance::KeepSubordinateIdMappingsForManualRecovery;
    plan.requires_consent = true;
    Ok(())
}

pub(super) fn install_service(
    plan: &mut RemediationPlan,
    recipe: LinuxRootlessRecipeV1,
) -> anyhow::Result<()> {
    validate_recipe(recipe)?;
    plan.effects.extend([
        PlanEffect::VerifyRootlesskitBeforeInstall,
        PlanEffect::RefuseConflictingExistingRootlessInstallation,
        PlanEffect::InstallAndEnableCurrentUserRootlessDockerService,
        PlanEffect::PreserveSystemRootfulDockerService,
    ]);
    plan.file_changes
        .push(ManagedFileChange::CurrentUserRootlessDockerUnit);
    plan.service_changes
        .push(ManagedServiceChange::CurrentUserRootlessDockerService);
    plan.precondition_probe_ids = host_preconditions(&[
        "host:linux:rootless_helpers",
        "host:linux:service_manager",
        "host:linux:user_runtime_directory",
        "host:linux:rootless_service_absent",
        "host:linux:rootlesskit_smoke",
        "docker:endpoint_environment",
    ]);
    plan.verification_probe_ids = vec![
        "host:linux:rootless_recipe".into(),
        "host:linux:rootless_helpers".into(),
        "host:linux:service_manager".into(),
        "docker:rootless".into(),
        "docker:rootless-smoke".into(),
    ];
    plan.required_consents
        .push(PlanConsent::RootfulDockerCoexistence);
    plan.privilege = PrivilegeLevel::CurrentUser;
    plan.rollback = RollbackGuidance::KeepRootlessUserStateForManualRecovery;
    plan.requires_consent = true;
    Ok(())
}

fn validate_recipe(recipe: LinuxRootlessRecipeV1) -> anyhow::Result<()> {
    anyhow::ensure!(
        matches!(
            recipe,
            LinuxRootlessRecipeV1::Debian13Amd64
                | LinuxRootlessRecipeV1::Ubuntu2404Amd64
                | LinuxRootlessRecipeV1::PopOs2404Amd64
                | LinuxRootlessRecipeV1::Fedora45Amd64
        ),
        "rootless Docker plan requires an exact supported distro recipe"
    );
    Ok(())
}

fn host_preconditions(operation: &[&str]) -> Vec<String> {
    SUPPORTED_HOST
        .iter()
        .chain(operation)
        .map(|probe| (*probe).to_owned())
        .collect()
}

#[cfg(test)]
mod tests {
    use crate::app::{
        bootstrap_remediation::{
            identity::HostIdentity,
            plan::{
                ManagedFileChange, ManagedPackageChange, PlanConsent, PlanEffect, RemediationPlan,
            },
            request::RemediationRequest,
        },
        bootstrap_repair_policy::{Host, Operation, Proposal},
    };

    fn plan(operation: Operation, prerequisite_id: &str) -> RemediationPlan {
        plan_for_os(
            operation,
            prerequisite_id,
            "ubuntu-24.04-amd64-kernel-6.8.0-31-generic",
        )
    }

    fn plan_for_os(
        operation: Operation,
        prerequisite_id: &str,
        os_version: &str,
    ) -> RemediationPlan {
        let identity =
            HostIdentity::from_facts(Host::Linux, os_version, "test-machine", "test-user").unwrap();
        let request = RemediationRequest::from_proposal_for_test(
            &Proposal {
                schema_version: 1,
                operation,
                host: Host::Linux,
                prerequisite_id: prerequisite_id.to_owned(),
                image_profile: None,
            },
            &identity,
        )
        .unwrap();
        RemediationPlan::build_for_test(&request, &identity).unwrap()
    }

    #[test]
    fn exact_distro_recipe_plans_choose_the_matching_package_manager_and_files() {
        for (os_version, expected_package, apt_recipe) in [
            (
                "debian-13-amd64-kernel-6.12.1",
                ManagedPackageChange::Debian,
                true,
            ),
            (
                "ubuntu-24.04-amd64-kernel-6.8.0",
                ManagedPackageChange::Ubuntu,
                true,
            ),
            (
                "pop-24.04-amd64-kernel-6.17.9",
                ManagedPackageChange::PopOs,
                true,
            ),
            (
                "fedora-45-amd64-kernel-6.17.9",
                ManagedPackageChange::Fedora,
                false,
            ),
        ] {
            let plan = plan_for_os(
                Operation::InstallLinuxRootlessPackages,
                "setup:linux:rootless:packages",
                os_version,
            );
            assert_eq!(plan.package_changes, vec![expected_package]);
            assert_eq!(
                plan.effects
                    .contains(&PlanEffect::ConfigureDockerAptRepository),
                apt_recipe
            );
            assert_eq!(
                plan.effects
                    .contains(&PlanEffect::ConfigureDockerDnfRepository),
                !apt_recipe
            );
            assert_eq!(
                plan.file_changes
                    .contains(&ManagedFileChange::DockerAptSourcesEntry),
                apt_recipe
            );
            assert_eq!(
                plan.file_changes
                    .contains(&ManagedFileChange::DockerDnfRepositoryEntry),
                !apt_recipe
            );
            assert_eq!(
                plan.required_consents
                    .contains(&PlanConsent::RootfulDockerPackageActivation),
                apt_recipe
            );
        }
    }

    #[test]
    fn mutating_operations_bind_supported_kernel_namespace_and_cgroup_probes() {
        let package = plan(
            Operation::InstallLinuxRootlessPackages,
            "setup:linux:rootless:packages",
        );
        let subids = plan(
            Operation::ConfigureLinuxSubordinateIds,
            "setup:linux:rootless:subids",
        );
        let service = plan(
            Operation::InstallLinuxRootlessService,
            "setup:linux:rootless:service",
        );
        let base = [
            "host:linux:rootless_recipe",
            "host:linux:kernel",
            "host:linux:user_namespaces",
            "host:linux:cgroups",
        ];
        let expected_base = base.map(str::to_owned);
        for remediation in [&package, &subids, &service] {
            assert_eq!(
                &remediation.precondition_probe_ids[..base.len()],
                expected_base.as_slice()
            );
        }
        assert_eq!(
            package.precondition_probe_ids.last().map(String::as_str),
            Some("host:linux:package_manager")
        );
        assert_eq!(
            &subids.precondition_probe_ids[base.len()..],
            ["host:linux:subuid", "host:linux:subgid"]
                .map(str::to_owned)
                .as_slice()
        );
        assert!(
            service
                .precondition_probe_ids
                .contains(&"host:linux:rootless_helpers".into())
        );
    }
}
