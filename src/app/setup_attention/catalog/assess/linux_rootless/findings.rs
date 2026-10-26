use super::super::super::Finding;
use super::super::super::dependencies;
use super::{PACKAGES_ID, SERVICE_ID, SUBIDS_ID, TaskReadiness};

pub(super) fn package(readiness: TaskReadiness) -> Finding {
    match readiness {
        TaskReadiness::Actionable => Finding::action(
            PACKAGES_ID,
            "Install rootless Docker prerequisites",
            "The experimental Linux distribution recipe is missing one or more required rootless Docker helper binaries.",
            "Install the required rootless Docker packages with the recipe's package manager, then rerun setup checks.",
        ),
        TaskReadiness::Manual | TaskReadiness::NotRequired => Finding::unverified(
            PACKAGES_ID,
            "Verify rootless Docker packages manually",
            "The package manager or rootless helper evidence is not reliable enough for an automatic package change.",
            "Review the rootless Docker package requirements for this distribution manually, then rerun setup checks.",
        ),
    }
}

pub(super) fn subids(readiness: TaskReadiness, prerequisites: Vec<String>) -> Finding {
    let finding = match readiness {
        TaskReadiness::Actionable => Finding::action(
            SUBIDS_ID,
            "Configure subordinate UID and GID ranges",
            "The current user is missing a valid subordinate UID or GID range for rootless Docker.",
            "Assign separate ranges of at least 65,536 subordinate IDs to this user, then rerun setup checks.",
        ),
        TaskReadiness::Manual | TaskReadiness::NotRequired => Finding::unverified(
            SUBIDS_ID,
            "Verify subordinate ID ranges manually",
            "The current user's subordinate UID or GID evidence is inaccessible, malformed, or contains an existing range this automatic repair cannot safely extend.",
            "Review both subordinate ID files manually; do not apply an automatic change over existing ranges.",
        ),
    };
    dependencies::requires(finding, prerequisites)
}

pub(super) fn service(readiness: TaskReadiness, prerequisites: Vec<String>) -> Finding {
    let finding = match readiness {
        TaskReadiness::Actionable => Finding::action(
            SERVICE_ID,
            "Install the current user's rootless Docker service",
            "The exact host recipe and systemd, cgroup, user namespace, package, and ID mapping prerequisites are verified.",
            "Install and enable the current user's rootless Docker service, then rerun setup checks.",
        ),
        TaskReadiness::Manual | TaskReadiness::NotRequired => Finding::unverified(
            SERVICE_ID,
            "Verify the rootless Docker service prerequisites manually",
            "The current user's systemd runtime or required Linux capabilities could not be confirmed.",
            "Review systemd, cgroup delegation, kernel, and user namespace evidence manually before installing the service.",
        ),
    };
    dependencies::requires(finding, prerequisites)
}
