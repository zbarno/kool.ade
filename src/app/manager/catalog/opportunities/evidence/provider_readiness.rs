use sha2::Digest;

use crate::{
    app::session::Project,
    core::product_capabilities::{ReadinessEvidence, ReadinessState},
    execution::contract::HarnessRole,
    persistence::harness_settings::{self, DetectedHarness, MANAGER},
};

const CAPABILITY_ID: &str = "provider-routing-and-role-isolation";
const PROBE_ID: &str = "manager-provider-route-v1";

pub(super) fn manager_route_evidence(project: &Project) -> Vec<ReadinessEvidence> {
    let (settings, diagnostic) = harness_settings::load();
    if diagnostic.is_some() {
        return Vec::new();
    }
    let Some((provider_id, model)) = crate::app::root::harness_selection::selected_harness_route(
        &settings,
        &project.state.harness_routes.routes,
        MANAGER,
    ) else {
        return evidence(
            "manager-route:unconfigured".as_bytes(),
            ReadinessState::NotReady,
        );
    };
    let Some(detected) = settings.discovered.get(provider_id) else {
        return evidence(
            format!("{provider_id}:discovery-missing").as_bytes(),
            ReadinessState::Unavailable,
        );
    };
    let state = readiness_state(
        provider_id,
        model,
        detected,
        harness_settings::current_unix_ms(),
    );
    let Ok(snapshot) = serde_json::to_vec(&(provider_id, model, detected)) else {
        return Vec::new();
    };
    evidence(&snapshot, state)
}

fn readiness_state(
    provider_id: &str,
    model: Option<&str>,
    detected: &DetectedHarness,
    now_unix_ms: u64,
) -> ReadinessState {
    if !detected.is_fresh_at(now_unix_ms) {
        return ReadinessState::Unavailable;
    }
    if !detected.ready || detected.configuration_required {
        return ReadinessState::NotReady;
    }
    let Some(snapshot) = detected.capability_snapshot.as_ref() else {
        return ReadinessState::Unavailable;
    };
    if snapshot.validate().is_err() || snapshot.provider_id != provider_id {
        return ReadinessState::Unavailable;
    }
    let profile_blocked = crate::harness::execution_security::profile_block_reason(
        provider_id,
        crate::harness::ExecutionProfile::Manager,
    )
    .is_some();
    if profile_blocked {
        return ReadinessState::NotReady;
    }
    if !snapshot.profile_evidence_is_current()
        || snapshot.cli_version != detected.version
        || !detected.has_current_authenticated_capability(provider_id, now_unix_ms)
    {
        return ReadinessState::Unavailable;
    }
    let role = snapshot
        .roles
        .iter()
        .find(|readiness| readiness.role == HarnessRole::Manager)
        .map(|readiness| readiness.available);
    let mode = snapshot
        .modes
        .iter()
        .find(|readiness| readiness.mode == HarnessRole::Manager.harness_mode())
        .map(|readiness| readiness.available);
    let observed = [
        snapshot.cli_binary,
        snapshot.authentication,
        snapshot.model_catalog,
        snapshot.scriptable_invocation,
        snapshot.docker_mcp_bridge,
        snapshot.execution_target,
        role.unwrap_or(crate::execution::contract::ReadinessState::Unknown),
        mode.unwrap_or(crate::execution::contract::ReadinessState::Unknown),
    ];
    if let Some(state) = observed.into_iter().find_map(assess_provider_observation) {
        return state;
    }
    if !snapshot.supports_role(HarnessRole::Manager) {
        return ReadinessState::NotReady;
    }
    let model_ready = model.map_or_else(
        || {
            detected
                .verified_default_model_at(provider_id, now_unix_ms)
                .is_some()
        },
        |model| detected.has_verified_model_at(provider_id, model, now_unix_ms),
    );
    if !detected.model_catalog_fresh || !model_ready {
        return ReadinessState::NotReady;
    }
    ReadinessState::Ready
}

fn assess_provider_observation(
    state: crate::execution::contract::ReadinessState,
) -> Option<ReadinessState> {
    match state {
        crate::execution::contract::ReadinessState::Ready => None,
        crate::execution::contract::ReadinessState::Unavailable => Some(ReadinessState::NotReady),
        crate::execution::contract::ReadinessState::Unknown => Some(ReadinessState::Unavailable),
    }
}

fn evidence(snapshot: &[u8], state: ReadinessState) -> Vec<ReadinessEvidence> {
    let Some((_, build_commit)) = crate::build_identity::current() else {
        return Vec::new();
    };
    let digest = format!("{:x}", sha2::Sha256::digest(snapshot));
    ReadinessEvidence::from_registered_probe_state(
        CAPABILITY_ID.into(),
        PROBE_ID.into(),
        build_commit.into(),
        digest,
        state,
    )
    .into_iter()
    .collect()
}

#[cfg(test)]
#[path = "provider_readiness_tests.rs"]
mod tests;
