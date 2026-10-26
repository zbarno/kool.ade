use super::super::HarnessProbe;

pub(in crate::app::dialogs::harness_setup) fn apply_probe_results(
    settings: &mut crate::persistence::harness_settings::HarnessSettings,
    reports: &[HarnessProbe],
    _preferred: Option<&str>,
    generation: u64,
) {
    for report in reports {
        update_settings_from_probe(settings, report, generation);
    }
}

pub(super) fn update_settings_from_probe(
    settings: &mut crate::persistence::harness_settings::HarnessSettings,
    report: &HarnessProbe,
    generation: u64,
) {
    let (capability_snapshot, profile_diagnostic) = snapshot_from_probe(report, generation);
    let diagnostic = [report.diagnostic.as_deref(), profile_diagnostic.as_deref()]
        .into_iter()
        .flatten()
        .filter(|message| !message.trim().is_empty())
        .collect::<Vec<_>>();
    settings.discovered.insert(
        report.id.clone(),
        crate::persistence::harness_settings::DetectedHarness {
            status: report.status.clone(),
            version: report.version.clone(),
            executable: report.executable.clone(),
            diagnostic: (!diagnostic.is_empty()).then(|| diagnostic.join("; ")),
            ready: report.ready
                && report.authentication == crate::execution::contract::ReadinessState::Ready,
            models: report.models.clone(),
            default_model: report.default_model.clone(),
            model_catalog_fresh: report.model_catalog_fresh,
            configuration_required: report.configuration_required,
            implementation_available: report.implementation_available
                && report.authentication == crate::execution::contract::ReadinessState::Ready,
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
            snapshot.docker_mcp_bridge = if implementation_available
                && snapshot.validate().is_ok()
                && snapshot.cli_binary == ReadinessState::Ready
                && snapshot
                    .cli_version
                    .as_deref()
                    .is_some_and(|version| !version.trim().is_empty())
                && snapshot.authentication == ReadinessState::Ready
                && snapshot.model_catalog == ReadinessState::Ready
                && snapshot.scriptable_invocation == ReadinessState::Ready
                && crate::harness::execution_security::supports_universal_role_contract(id)
            {
                ReadinessState::Ready
            } else {
                ReadinessState::Unavailable
            };
            for role in &mut snapshot.roles {
                if role.role.requires_execution_target() {
                    role.available = if snapshot.execution_target == ReadinessState::Ready
                        && snapshot.cli_binary == ReadinessState::Ready
                        && snapshot.authentication == ReadinessState::Ready
                        && snapshot.model_catalog == ReadinessState::Ready
                        && snapshot.scriptable_invocation == ReadinessState::Ready
                        && crate::harness::execution_security::supports_mode_contract(
                            id,
                            role.role.harness_mode(),
                        ) {
                        ReadinessState::Ready
                    } else {
                        ReadinessState::Unavailable
                    };
                }
            }
        }
    }
    for report in reports {
        report.implementation_available = report.ready
            && report.authentication == ReadinessState::Ready
            && implementation_available;
    }
}

fn snapshot_from_probe(
    report: &HarnessProbe,
    generation: u64,
) -> (
    crate::execution::contract::HarnessCapabilitySnapshotV1,
    Option<String>,
) {
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
    let (scriptable_invocation, profile_diagnostic) =
        match (report.executable.as_deref(), report.version.as_deref()) {
            (Some(executable), Some(version)) => {
                let path = std::path::Path::new(executable);
                let profile = if path.is_file() {
                    crate::harness::execution_security::verify_scriptable_invocation(
                        &report.id, path, version,
                    )
                } else {
                    Err(anyhow::anyhow!("discovered CLI executable is not a file"))
                };
                match profile {
                    Ok(()) => (ReadinessState::Ready, None),
                    Err(error) => (
                        ReadinessState::Unavailable,
                        Some(crate::error::redact_secrets(&error.to_string())),
                    ),
                }
            }
            _ => (ReadinessState::Unknown, None),
        };
    let adapter_contract =
        if crate::harness::execution_security::supports_universal_role_contract(&report.id) {
            ReadinessState::Ready
        } else {
            ReadinessState::Unavailable
        };
    let provider_capability = if [
        cli_binary,
        authentication,
        model_catalog,
        scriptable_invocation,
        adapter_contract,
    ]
    .contains(&ReadinessState::Unavailable)
    {
        ReadinessState::Unavailable
    } else if [
        cli_binary,
        authentication,
        model_catalog,
        scriptable_invocation,
        adapter_contract,
    ]
    .contains(&ReadinessState::Unknown)
    {
        ReadinessState::Unknown
    } else {
        ReadinessState::Ready
    };

    let execution_target = if report.implementation_available {
        ReadinessState::Ready
    } else {
        ReadinessState::Unavailable
    };

    (
        HarnessCapabilitySnapshotV1 {
            schema_version: 1,
            provider_id: report.id.clone(),
            probe_id: uuid::Uuid::new_v4().hyphenated().to_string(),
            probe_cancellation_generation: generation,
            probe_sequence: generation,
            cli_binary,
            cli_version: report.version.clone(),
            authentication,
            model_catalog,
            scriptable_invocation,
            // This is the app-owned, provider-specific MCP adapter contract.
            // Runtime Docker availability is tracked separately below.
            docker_mcp_bridge: if provider_capability == ReadinessState::Ready
                && execution_target == ReadinessState::Ready
            {
                ReadinessState::Ready
            } else {
                ReadinessState::Unavailable
            },
            execution_target,
            failure: None,
            modes: ExecutionMode::ALL
                .into_iter()
                .map(|mode| ModeReadinessV1 {
                    mode,
                    available: if provider_capability != ReadinessState::Ready {
                        provider_capability
                    } else if crate::harness::execution_security::supports_mode_contract(
                        &report.id, mode,
                    ) {
                        ReadinessState::Ready
                    } else {
                        ReadinessState::Unavailable
                    },
                    failure: None,
                })
                .collect(),
            roles: HarnessRole::ALL
                .into_iter()
                .map(|role| RoleReadinessV1 {
                    role,
                    available: if provider_capability != ReadinessState::Ready {
                        provider_capability
                    } else if !crate::harness::execution_security::supports_mode_contract(
                        &report.id,
                        role.harness_mode(),
                    ) || (role.requires_execution_target()
                        && execution_target != ReadinessState::Ready)
                    {
                        ReadinessState::Unavailable
                    } else {
                        ReadinessState::Ready
                    },
                    failure: None,
                })
                .collect(),
        },
        profile_diagnostic,
    )
}
