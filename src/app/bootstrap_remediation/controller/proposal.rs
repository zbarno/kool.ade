use crate::app::bootstrap_repair_policy::{Host, Operation, Proposal};

pub(super) fn proposal_for_task(task_id: &str) -> Option<Proposal> {
    let operation = match task_id {
        "setup:linux:rootless:packages" => Operation::InstallLinuxRootlessPackages,
        "setup:linux:rootless:subids" => Operation::ConfigureLinuxSubordinateIds,
        "setup:linux:rootless:service" => Operation::InstallLinuxRootlessService,
        _ => return None,
    };
    Some(Proposal {
        schema_version: 1,
        operation,
        host: Host::Linux,
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
    )
}
