use crate::app::{
    bootstrap_repair_policy::{Host, Operation, Proposal},
    setup_attention::catalog::LinuxRootlessRecipeV1,
};

pub(super) fn proposal_for_task(task_id: &str) -> Option<Proposal> {
    let operation = match task_id {
        "setup:linux:rootless:packages" => Operation::InstallLinuxRootlessPackages,
        "setup:computer:qemu" => Operation::InstallLinuxQemu,
        "setup:linux:rootless:subids" => Operation::ConfigureLinuxSubordinateIds,
        "setup:linux:rootless:service" => Operation::InstallLinuxRootlessService,
        "setup:windows:wsl-features" => Operation::EnableWindowsWslFeatures,
        _ => return None,
    };
    let host = if operation == Operation::EnableWindowsWslFeatures {
        Host::Windows
    } else {
        Host::Linux
    };
    Some(Proposal {
        schema_version: 1,
        operation,
        host,
        prerequisite_id: task_id.to_owned(),
        image_profile: None,
    })
}

pub(super) fn is_supported_task(task_id: &str) -> bool {
    matches!(
        task_id,
        "setup:linux:rootless:packages"
            | "setup:linux:rootless:subids"
            | "setup:linux:rootless:service"
    ) || cfg!(windows)
        && task_id == "setup:windows:wsl-features"
        && crate::windows_setup_helper::publisher_trust_configured()
        || (task_id == "setup:computer:qemu" && current_qemu_recipe().is_some())
}

fn current_qemu_recipe() -> Option<LinuxRootlessRecipeV1> {
    #[cfg(target_os = "linux")]
    {
        let identity = crate::app::bootstrap_remediation::identity::HostIdentity::current().ok()?;
        let recipe = LinuxRootlessRecipeV1::from_os_version(identity.os_version())?;
        matches!(
            recipe,
            LinuxRootlessRecipeV1::Debian13Amd64
                | LinuxRootlessRecipeV1::Ubuntu2404Amd64
                | LinuxRootlessRecipeV1::PopOs2404Amd64
        )
        .then_some(recipe)
    }
    #[cfg(not(target_os = "linux"))]
    {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn windows_wsl_task_maps_to_fixed_proposal_but_stays_out_of_live_ui() {
        let proposal = proposal_for_task("setup:windows:wsl-features").unwrap();
        assert_eq!(proposal.operation, Operation::EnableWindowsWslFeatures);
        assert_eq!(proposal.host, Host::Windows);
        assert_eq!(proposal.prerequisite_id, "setup:windows:wsl-features");
        assert!(proposal.image_profile.is_none());
        assert_eq!(
            is_supported_task("setup:windows:wsl-features"),
            cfg!(windows) && crate::windows_setup_helper::publisher_trust_configured()
        );
    }

    #[test]
    fn unsupported_windows_repairs_do_not_map_or_enter_live_ui() {
        for task_id in [
            "setup:windows:wsl-kernel",
            "setup:windows:reboot",
            "setup:windows:virtualization",
            "setup:windows:wsl-policy",
        ] {
            assert!(proposal_for_task(task_id).is_none());
            assert!(!is_supported_task(task_id));
        }
    }

    #[test]
    fn qemu_install_action_is_available_only_for_a_supported_linux_recipe() {
        assert_eq!(
            proposal_for_task("setup:computer:qemu").unwrap().operation,
            Operation::InstallLinuxQemu
        );
        assert_eq!(
            is_supported_task("setup:computer:qemu"),
            current_qemu_recipe().is_some()
        );
    }
}
