use super::*;

pub(super) fn provider_ready(snapshot: &MachineSnapshotV1, provider: ProviderIdV1) -> bool {
    [
        Capability::ProviderExecutable,
        Capability::ProviderVersion,
        Capability::ProviderAuthentication,
        Capability::ProviderModels,
        Capability::ProviderContextOnly,
    ]
    .into_iter()
    .all(|capability| {
        super::current_ready(
            snapshot,
            Domain::Provider,
            capability,
            Subject::Provider(provider),
            true,
        )
    })
}

pub(super) fn provider_scan_current(snapshot: &MachineSnapshotV1, provider: ProviderIdV1) -> bool {
    let items = snapshot
        .probe_evidence
        .iter()
        .filter(|item| {
            item.prerequisite.domain == Domain::Provider
                && item.prerequisite.subject == Subject::Provider(provider)
        })
        .collect::<Vec<_>>();
    items.len() == 5
        && items.iter().all(|item| {
            item.scan_generation == snapshot.scan_generation
                && item.source_generation == Some(snapshot.scan_generation)
                && evidence::provider_evidence(
                    item.prerequisite.capability,
                    item.state,
                    item.origin,
                    item.code,
                )
        })
        && [
            Capability::ProviderExecutable,
            Capability::ProviderVersion,
            Capability::ProviderAuthentication,
            Capability::ProviderModels,
            Capability::ProviderContextOnly,
        ]
        .into_iter()
        .all(|capability| {
            items
                .iter()
                .any(|item| item.prerequisite.capability == capability)
        })
}

pub(super) fn explicit_provider_choice_current(snapshot: &MachineSnapshotV1) -> bool {
    snapshot
        .setup_provider
        .into_iter()
        .chain(snapshot.selected_provider)
        .any(|provider| provider_scan_current(snapshot, provider))
}

pub(super) fn missing_setup_provider_has_verification_finding(
    snapshot: &MachineSnapshotV1,
    provider: ProviderIdV1,
) -> bool {
    let finding_id = format!("setup:cli:{}:verify", provider.probe_id());
    let current_source_evidence_exists = snapshot.probe_evidence.iter().any(|item| {
        item.prerequisite.domain == Domain::Provider
            && item.prerequisite.subject == Subject::Provider(provider)
            && item.source_generation == Some(snapshot.scan_generation)
    });
    !current_source_evidence_exists
        && snapshot.findings.iter().any(|finding| {
            finding.id == finding_id
                && finding.status == crate::app::setup_attention::catalog::FindingStatus::Unverified
        })
}
