use crate::app::bootstrap_repair_policy::{Host, Operation, Proposal};

pub(super) fn proposal_for_task(task_id: &str) -> Option<Proposal> {
    let operation = match task_id {
        "setup:linux:rootless:packages" => Operation::InstallLinuxRootlessPackages,
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
}
