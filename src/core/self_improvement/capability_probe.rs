use serde::Deserialize;
use sha2::{Digest, Sha256};

const MAX_MANIFEST_BYTES: usize = 64 * 1024;
const MAX_CAPABILITIES: usize = 512;
const MANIFEST: &str =
    include_str!("../../../.koolade-packet/planning/product/active-capabilities.json");

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct ActiveCapabilitiesV1 {
    schema_version: u16,
    capabilities: Vec<ActiveCapabilityV1>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct ActiveCapabilityV1 {
    capability_id: String,
    readiness_probe_id: String,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(super) struct ActiveCapabilityProbeEvidence {
    pub capability_id: String,
    pub readiness_probe_id: String,
    pub manifest_sha256: String,
}

pub(super) fn probe_compiled_capability(
    run: &crate::core::self_improvement::SelfImprovementRunV1,
) -> Result<ActiveCapabilityProbeEvidence, String> {
    probe_capability_manifest_for_run(run, MANIFEST)
}

pub(super) fn probe_capability_manifest_for_run(
    run: &crate::core::self_improvement::SelfImprovementRunV1,
    manifest_json: &str,
) -> Result<ActiveCapabilityProbeEvidence, String> {
    let capability_id = run
        .capability_id
        .as_deref()
        .ok_or_else(|| "approved plan has no stable product capability ID".to_owned())?;
    let capability = probe_capability_manifest(capability_id, manifest_json)?;
    run_registered_probe(
        run,
        &capability.capability_id,
        &capability.readiness_probe_id,
    )?;
    Ok(capability)
}

fn run_registered_probe(
    run: &crate::core::self_improvement::SelfImprovementRunV1,
    capability_id: &str,
    readiness_probe_id: &str,
) -> Result<(), String> {
    match (capability_id, readiness_probe_id) {
        ("self-improvement-lifecycle", "self-improvement-lifecycle-v1") => {
            probe_self_improvement_lifecycle(run)
        }
        _ => Err("active capability references an unregistered readiness probe".into()),
    }
}

fn probe_self_improvement_lifecycle(
    run: &crate::core::self_improvement::SelfImprovementRunV1,
) -> Result<(), String> {
    use crate::core::self_improvement::{SelfImprovementEventV1, SelfImprovementPhaseV1};

    run.validate()?;
    if run.phase != SelfImprovementPhaseV1::InstalledNotActivated {
        return Err(
            "self-improvement lifecycle capability requires an installed inactive run".into(),
        );
    }
    if run.code_review_uids.is_empty() {
        return Err("active self-improvement capability has no code review receipt".into());
    }
    if run.runtime_qa_uids.is_empty() {
        return Err("active self-improvement capability has no runtime QA receipt".into());
    }
    if run.spec_receipt_uid.is_none() {
        return Err("active self-improvement capability has no specification receipt".into());
    }
    let mut transition_probe = run.clone();
    if transition_probe
        .apply(run.revision, SelfImprovementEventV1::NeedsAttention)
        .is_ok()
    {
        return Err("self-improvement terminal state guard failed its active probe".into());
    }
    Ok(())
}

fn probe_capability_manifest(
    capability_id: &str,
    manifest_json: &str,
) -> Result<ActiveCapabilityProbeEvidence, String> {
    if !valid_slug(capability_id) || manifest_json.len() > MAX_MANIFEST_BYTES {
        return Err("active capability probe input is invalid or exceeds its size limit".into());
    }
    let manifest: ActiveCapabilitiesV1 = serde_json::from_str(manifest_json)
        .map_err(|error| format!("compiled active capability manifest is invalid: {error}"))?;
    if manifest.schema_version != 1 || manifest.capabilities.len() > MAX_CAPABILITIES {
        return Err("compiled active capability manifest has an unsupported schema or size".into());
    }
    let mut capability_ids = std::collections::BTreeSet::new();
    let mut selected = None;
    for capability in manifest.capabilities {
        if !valid_slug(&capability.capability_id)
            || !valid_slug(&capability.readiness_probe_id)
            || !registered_probe_pair(&capability.capability_id, &capability.readiness_probe_id)
            || !capability_ids.insert(capability.capability_id.clone())
        {
            return Err(
                "compiled active capability manifest has an invalid or duplicate entry".into(),
            );
        }
        if capability.capability_id == capability_id {
            selected = Some(capability);
        }
    }
    let selected = selected.ok_or_else(|| {
        "the active application does not register this feature as a ready product capability"
            .to_owned()
    })?;
    Ok(ActiveCapabilityProbeEvidence {
        capability_id: selected.capability_id,
        readiness_probe_id: selected.readiness_probe_id,
        manifest_sha256: format!("{:x}", Sha256::digest(manifest_json.as_bytes())),
    })
}

fn registered_probe_pair(capability_id: &str, readiness_probe_id: &str) -> bool {
    matches!(
        (capability_id, readiness_probe_id),
        (
            "self-improvement-lifecycle",
            "self-improvement-lifecycle-v1"
        )
    )
}

pub(super) fn valid_capability_id(value: &str) -> bool {
    valid_slug(value)
}

fn valid_slug(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 96
        && value.bytes().all(|byte| {
            byte.is_ascii_lowercase() || byte.is_ascii_digit() || byte == b'-' || byte == b'.'
        })
        && value
            .bytes()
            .next()
            .is_some_and(|byte| byte.is_ascii_lowercase())
}

#[cfg(test)]
#[path = "capability_probe/tests.rs"]
mod tests;
