use super::super::HarnessProbe;

pub(super) fn failed_probe(id: &str, message: &str) -> HarnessProbe {
    HarnessProbe {
        id: id.into(),
        status: format!("{id} (probe failed)"),
        version: None,
        executable: None,
        diagnostic: Some(message.into()),
        ready: false,
        models: Vec::new(),
        default_model: None,
        model_catalog_fresh: false,
        configuration_required: false,
        authentication: crate::execution::contract::ReadinessState::Unknown,
        implementation_available: false,
    }
}

pub(in crate::app::dialogs::harness_setup) fn apply_probe_results(
    settings: &mut crate::persistence::harness_settings::HarnessSettings,
    reports: &[HarnessProbe],
    preferred: Option<&str>,
    generation: u64,
) {
    for report in reports {
        update_settings_from_probe(settings, report, generation);
    }
    if settings.default_harness.is_none() {
        settings.default_harness = preferred
            .filter(|id| {
                settings.discovered.get(*id).is_some_and(|entry| {
                    super::super::has_current_provider_capabilities(
                        id,
                        entry,
                        crate::persistence::harness_settings::current_unix_ms(),
                    )
                })
            })
            .map(str::to_owned)
            .or_else(|| {
                [
                    "pi",
                    "codex",
                    "claude",
                    "opencode",
                    "copilot",
                    "antigravity",
                ]
                .into_iter()
                .find_map(|id| {
                    settings
                        .discovered
                        .get(id)
                        .is_some_and(|entry| {
                            super::super::has_current_provider_capabilities(
                                id,
                                entry,
                                crate::persistence::harness_settings::current_unix_ms(),
                            )
                        })
                        .then(|| id.to_owned())
                })
            });
    }
}

pub(super) fn update_settings_from_probe(
    settings: &mut crate::persistence::harness_settings::HarnessSettings,
    report: &HarnessProbe,
    generation: u64,
) {
    let capability_snapshot = snapshot_from_probe(report, generation);
    settings.discovered.insert(
        report.id.clone(),
        crate::persistence::harness_settings::DetectedHarness {
            status: report.status.clone(),
            version: report.version.clone(),
            executable: report.executable.clone(),
            diagnostic: report.diagnostic.clone(),
            ready: report.ready,
            models: report.models.clone(),
            default_model: report.default_model.clone(),
            model_catalog_fresh: report.model_catalog_fresh,
            configuration_required: report.configuration_required,
            implementation_available: report.implementation_available,
            checked_at_unix_ms: crate::persistence::harness_settings::current_unix_ms(),
            capability_snapshot: Some(capability_snapshot),
        },
    );
}

pub(in crate::app::dialogs::harness_setup) fn update_runtime_readiness(
    settings: &mut crate::persistence::harness_settings::HarnessSettings,
    reports: &mut [HarnessProbe],
    implementation_available: bool,
) {
    use crate::execution::contract::ReadinessState;

    // Runtime availability is shared by every provider. Retained provider
    // snapshots keep their own discovery generation, while this global field
    // is refreshed for all of them by the current, generation-fenced message.
    for (id, entry) in &mut settings.discovered {
        if let Some(snapshot) = entry.capability_snapshot.as_mut()
            && snapshot.provider_id == *id
        {
            snapshot.execution_target = if implementation_available {
                ReadinessState::Ready
            } else {
                ReadinessState::Unavailable
            };
        }
    }
    for report in reports {
        report.implementation_available = report.ready && implementation_available;
    }
}

fn snapshot_from_probe(
    report: &HarnessProbe,
    generation: u64,
) -> crate::execution::contract::HarnessCapabilitySnapshotV1 {
    use crate::execution::contract::{
        HarnessCapabilitySnapshotV1, HarnessRole, ModeReadinessV1, ReadinessState, RoleReadinessV1,
    };
    use crate::harness::ExecutionMode;

    let cli_binary = if report.executable.is_some() {
        ReadinessState::Ready
    } else {
        ReadinessState::Unavailable
    };
    let authentication = report.authentication;
    let model_catalog = match super::super::probe_result::model_catalog_status(report) {
        crate::harness::discovery::ModelCatalogStatus::Fresh => ReadinessState::Ready,
        crate::harness::discovery::ModelCatalogStatus::Stale
        | crate::harness::discovery::ModelCatalogStatus::Unavailable => ReadinessState::Unavailable,
        crate::harness::discovery::ModelCatalogStatus::NotChecked => ReadinessState::Unknown,
    };

    HarnessCapabilitySnapshotV1 {
        schema_version: 1,
        provider_id: report.id.clone(),
        probe_id: uuid::Uuid::new_v4().hyphenated().to_string(),
        probe_cancellation_generation: generation,
        probe_sequence: 1,
        cli_binary,
        cli_version: report.version.clone(),
        authentication,
        model_catalog,
        // Current discovery does not prove the provider's non-interactive
        // request/response contract for every mode.
        scriptable_invocation: ReadinessState::Unknown,
        // Discovery does not establish a live provider session or bridge.
        docker_mcp_bridge: ReadinessState::Unavailable,
        // Runtime readiness arrives independently from provider discovery.
        execution_target: ReadinessState::Unknown,
        failure: None,
        modes: ExecutionMode::ALL
            .into_iter()
            .map(|mode| ModeReadinessV1 {
                mode,
                available: ReadinessState::Unknown,
                failure: None,
            })
            .collect(),
        roles: HarnessRole::ALL
            .into_iter()
            .map(|role| RoleReadinessV1 {
                role,
                available: ReadinessState::Unknown,
                failure: None,
            })
            .collect(),
    }
}
