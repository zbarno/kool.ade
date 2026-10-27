use crate::app::setup_attention::catalog::{
    EvidenceCode as Code, EvidenceState as State, ImageArchitectureV1, LinuxDistributionV1,
    LinuxDistributionVersionV1, LinuxPackageManagerV1, LinuxRootlessRecipeV1,
    PrerequisiteCapability as Capability, ProbeEvidenceV1 as Evidence,
    ProbeObservationV1 as Observation,
};

use super::super::super::MachineSnapshotV1;
use super::host_fact;

pub(super) fn validate(snapshot: &MachineSnapshotV1) -> anyhow::Result<()> {
    validate_recipe(snapshot)?;
    validate_kernel(host_fact(snapshot, Capability::LinuxKernel)?)?;
    validate_user_namespaces(host_fact(snapshot, Capability::LinuxUserNamespaces)?)?;
    validate_subordinate_ids(host_fact(snapshot, Capability::LinuxSubuid)?)?;
    validate_subordinate_ids(host_fact(snapshot, Capability::LinuxSubgid)?)?;
    validate_helpers(host_fact(snapshot, Capability::LinuxRootlessHelpers)?)?;
    validate_cgroups(host_fact(snapshot, Capability::LinuxCgroups)?)?;
    validate_service_manager(host_fact(snapshot, Capability::LinuxServiceManager)?)?;
    validate_package_manager(snapshot)?;
    Ok(())
}

fn validate_recipe(snapshot: &MachineSnapshotV1) -> anyhow::Result<()> {
    let evidence = host_fact(snapshot, Capability::LinuxRootlessRecipe)?;
    let Some(Observation::LinuxRootlessRecipe {
        candidate,
        distribution_state,
    }) = &evidence.observation
    else {
        anyhow::bail!("rootless recipe evidence lacks its typed observation");
    };
    let expected = exact_recipe(
        snapshot.capability_matrix.linux_distribution_id,
        snapshot.capability_matrix.linux_distribution_version,
        snapshot.capability_matrix.observed_host_architecture,
    );
    anyhow::ensure!(
        *candidate == expected,
        "rootless recipe candidate conflicts with the host matrix"
    );
    match (candidate, evidence.state, evidence.code, distribution_state) {
        (Some(_), State::Ready, Code::LinuxRootlessRecipeCandidate, State::Ready) => {}
        (None, State::Unknown, Code::LinuxRootlessRecipeUnverified, State::Ready) => {}
        (None, State::Missing, Code::LinuxHostCheckMissing, State::Missing) => {}
        (None, State::Unknown, Code::LinuxProbeInaccessible, State::Unknown) => {}
        (None, State::Failed, Code::LinuxProbeMalformed, State::Failed) => {}
        _ => anyhow::bail!("rootless recipe status conflicts with its typed observation"),
    }
    Ok(())
}

fn exact_recipe(
    distribution: Option<LinuxDistributionV1>,
    version: Option<LinuxDistributionVersionV1>,
    architecture: ImageArchitectureV1,
) -> Option<LinuxRootlessRecipeV1> {
    LinuxRootlessRecipeV1::for_host(distribution, version, architecture)
}

fn validate_kernel(evidence: &Evidence) -> anyhow::Result<()> {
    match (&evidence.state, &evidence.observation) {
        (State::Ready, Some(Observation::LinuxKernel { version })) => {
            anyhow::ensure!(version.major > 0, "kernel version observation is malformed");
            Ok(())
        }
        (State::Missing | State::Unknown | State::Failed, None) => Ok(()),
        _ => anyhow::bail!("kernel probe status conflicts with its typed observation"),
    }
}

fn validate_user_namespaces(evidence: &Evidence) -> anyhow::Result<()> {
    let Some(Observation::LinuxUserNamespaces {
        max_user_namespaces,
        unprivileged_clone_allowed,
        apparmor_userns_restricted,
    }) = &evidence.observation
    else {
        anyhow::bail!("user namespace evidence lacks its typed observation");
    };
    match evidence.state {
        State::Ready => anyhow::ensure!(
            max_user_namespaces.is_some_and(|limit| limit > 0)
                && *unprivileged_clone_allowed != Some(false)
                && *apparmor_userns_restricted != Some(true),
            "ready user namespace probe has a denying policy"
        ),
        State::Missing => anyhow::ensure!(
            max_user_namespaces.is_none(),
            "missing user namespace evidence has a configured namespace limit"
        ),
        State::Unsupported => anyhow::ensure!(
            *max_user_namespaces == Some(0) || *unprivileged_clone_allowed == Some(false),
            "unsupported user namespace evidence lacks an explicit disabled setting"
        ),
        State::Unknown | State::Failed => {}
        _ => anyhow::bail!("user namespace probe has an invalid Linux state"),
    }
    Ok(())
}

fn validate_subordinate_ids(evidence: &Evidence) -> anyhow::Result<()> {
    let Some(Observation::LinuxSubordinateIds { configured }) = &evidence.observation else {
        anyhow::bail!("subordinate ID evidence lacks its typed observation");
    };
    anyhow::ensure!(
        *configured == (evidence.state == State::Ready),
        "subordinate ID status conflicts with its typed observation"
    );
    Ok(())
}

fn validate_helpers(evidence: &Evidence) -> anyhow::Result<()> {
    let Some(Observation::LinuxRootlessHelpers {
        newuidmap,
        newgidmap,
        rootlesskit,
        dockerd_rootless,
        setuptool,
        ..
    }) = &evidence.observation
    else {
        anyhow::bail!("rootless helper evidence lacks its typed observation");
    };
    let all_required = [
        *newuidmap,
        *newgidmap,
        *rootlesskit,
        *dockerd_rootless,
        *setuptool,
    ]
    .into_iter()
    .all(|value| value);
    if evidence.state == State::Ready {
        anyhow::ensure!(
            all_required,
            "ready rootless helper evidence has a missing binary"
        );
    }
    if evidence.state == State::Missing {
        anyhow::ensure!(
            !all_required,
            "missing rootless helper evidence has all binaries"
        );
    }
    Ok(())
}

fn validate_cgroups(evidence: &Evidence) -> anyhow::Result<()> {
    let Some(Observation::LinuxCgroups {
        cgroup_v2,
        cpu_delegated,
        memory_delegated,
        pids_delegated,
    }) = &evidence.observation
    else {
        anyhow::bail!("cgroup evidence lacks its typed observation");
    };
    if evidence.state == State::Ready {
        anyhow::ensure!(
            *cgroup_v2 && *cpu_delegated && *memory_delegated && *pids_delegated,
            "ready cgroup evidence lacks delegated v2 controllers"
        );
    }
    if evidence.state == State::Unsupported {
        anyhow::ensure!(!cgroup_v2, "unsupported cgroup evidence claims cgroup v2");
    }
    Ok(())
}

fn validate_service_manager(evidence: &Evidence) -> anyhow::Result<()> {
    let Some(Observation::LinuxServiceManager {
        runtime_directory_ready,
        private_socket_ready,
        bus_socket_ready,
    }) = &evidence.observation
    else {
        anyhow::bail!("service manager evidence lacks its typed observation");
    };
    let all_ready = *runtime_directory_ready && *private_socket_ready && *bus_socket_ready;
    if evidence.state == State::Ready {
        anyhow::ensure!(
            all_ready,
            "ready systemd evidence has an invalid runtime socket"
        );
    }
    if evidence.state == State::Missing {
        anyhow::ensure!(
            !all_ready,
            "missing systemd evidence has all runtime sockets"
        );
    }
    Ok(())
}

fn validate_package_manager(snapshot: &MachineSnapshotV1) -> anyhow::Result<()> {
    let evidence = host_fact(snapshot, Capability::LinuxPackageManager)?;
    let Some(Observation::LinuxPackageManager { expected, found }) = &evidence.observation else {
        anyhow::bail!("package manager evidence lacks its typed observation");
    };
    let expected_from_distro = snapshot
        .capability_matrix
        .linux_distribution_id
        .and_then(LinuxPackageManagerV1::for_distribution);
    anyhow::ensure!(
        *expected == expected_from_distro,
        "package manager expectation conflicts with the exact distribution ID"
    );
    match evidence.state {
        State::Ready => anyhow::ensure!(
            expected.is_some() && found == expected,
            "ready package manager evidence does not match the detected distribution"
        ),
        State::Missing | State::Unknown | State::Failed => anyhow::ensure!(
            found.is_none(),
            "non-ready package manager evidence reports a matching executable"
        ),
        _ => anyhow::bail!("package manager probe has an invalid Linux state"),
    }
    Ok(())
}
