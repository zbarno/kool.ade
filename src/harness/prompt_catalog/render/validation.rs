use super::super::{EvidenceConfidentiality, PromptRole, RoleInputV1, RolePayloadV1};

const MAX_FIELD_BYTES: usize = 16 * 1024;

pub(super) fn validate_input(input: &RoleInputV1) -> Result<(), String> {
    if input.output_schema_version != input.role.output_schema_version() {
        return Err(format!(
            "{} requires output schema {}",
            input.role.id(),
            input.role.output_schema_version()
        ));
    }
    for (name, value) in [
        ("workspace_uid", input.workspace_uid.as_str()),
        ("feature_uid", input.feature_uid.as_str()),
        ("original_goal", input.original_goal.as_str()),
        ("execution_policy_ref", input.execution_policy_ref.as_str()),
    ] {
        if value.trim().is_empty() {
            return Err(format!("{name} is required"));
        }
        if value.len() > MAX_FIELD_BYTES {
            return Err(format!("{name} exceeds {MAX_FIELD_BYTES} bytes"));
        }
    }
    if input
        .task_uid
        .as_ref()
        .is_some_and(|value| value.is_empty() || value.len() > 128)
        || input
            .approved_spec_ref
            .as_ref()
            .is_some_and(|value| value.is_empty() || value.len() > 1024)
        || input.approved_spec_hash.as_ref().is_some_and(|value| {
            value.len() != 64 || !value.bytes().all(|byte| byte.is_ascii_hexdigit())
        })
        || input.decision_refs.len() > 256
        || input
            .decision_refs
            .iter()
            .any(|value| value.trim().is_empty() || value.len() > 128)
    {
        return Err("common task identity or decision references exceed supported bounds".into());
    }
    super::super::validate_payload(&input.payload)?;
    if input.evidence_manifest.len() > 128 {
        return Err("evidence manifest exceeds 128 entries".into());
    }
    let mut evidence_ids = std::collections::HashSet::new();
    for evidence in &input.evidence_manifest {
        if evidence.id.trim().is_empty()
            || evidence.id.len() > 128
            || evidence.source.trim().is_empty()
            || !evidence_ids.insert(&evidence.id)
        {
            return Err("evidence IDs and sources must be nonempty and IDs unique".into());
        }
        if evidence.workspace_uid != input.workspace_uid {
            return Err("evidence belongs to a different workspace".into());
        }
        if evidence.confidentiality == EvidenceConfidentiality::Restricted {
            return Err("restricted evidence cannot be placed in a model prompt".into());
        }
        if evidence.content.len() > MAX_FIELD_BYTES {
            return Err("individual evidence item exceeds the prompt size limit".into());
        }
        if evidence.source.len() > 1024 {
            return Err("evidence source exceeds the prompt size limit".into());
        }
    }
    let payload_matches = matches!(
        (input.role, &input.payload),
        (
            PromptRole::Manager
                | PromptRole::ManagerV6
                | PromptRole::ManagerV5
                | PromptRole::ManagerV4
                | PromptRole::ManagerV3
                | PromptRole::ManagerV2,
            RolePayloadV1::Manager { .. }
        ) | (
            PromptRole::Planner,
            RolePayloadV1::PlannerOpportunity { .. }
        ) | (PromptRole::PlannerV2, RolePayloadV1::Planner { .. })
            | (
                PromptRole::PlannerConversation,
                RolePayloadV1::PlannerTurn { .. }
            )
            | (PromptRole::PlanReviewer, RolePayloadV1::Review { .. })
            | (
                PromptRole::CodeReviewer | PromptRole::CodeReviewerV1,
                RolePayloadV1::Review { .. }
            )
            | (
                PromptRole::RuntimeQaReviewer | PromptRole::RuntimeQaReviewerV2,
                RolePayloadV1::Review { .. }
            )
            | (
                PromptRole::RuntimeQaScenarioSynthesizer,
                RolePayloadV1::QaScenarioSynthesis { .. }
            )
            | (PromptRole::Decomposer, RolePayloadV1::Decomposer { .. })
            | (PromptRole::Implementer, RolePayloadV1::Worker { .. })
            | (PromptRole::RepairWorker, RolePayloadV1::Repair { .. })
            | (PromptRole::SpecReconciler, RolePayloadV1::SpecSync { .. })
            | (PromptRole::PrPreparer, RolePayloadV1::Pr { .. })
    );
    if !payload_matches {
        return Err("role and typed task payload do not match".into());
    }
    if let RolePayloadV1::Review {
        subject_hash,
        author_run_id,
        reviewer_run_id,
        author_session_id,
        reviewer_session_id,
        ..
    } = &input.payload
    {
        if input.role != PromptRole::PlanReviewer
            && input.role != PromptRole::CodeReviewer
            && input.role != PromptRole::CodeReviewerV1
            && input.role != PromptRole::RuntimeQaReviewer
            && input.role != PromptRole::RuntimeQaReviewerV2
        {
            return Err("review payload is only valid for reviewer roles".into());
        }
        if subject_hash.len() != 64 || !subject_hash.bytes().all(|byte| byte.is_ascii_hexdigit()) {
            return Err("review subject_hash must be a SHA-256 hex digest".into());
        }
        if author_run_id.is_empty()
            || reviewer_run_id.is_empty()
            || author_run_id == reviewer_run_id
        {
            return Err("reviewer run must be distinct from the author run".into());
        }
        let valid_session = |id: &Option<String>| {
            id.as_ref().is_some_and(|value| {
                uuid::Uuid::parse_str(value)
                    .is_ok_and(|parsed| parsed.hyphenated().to_string() == *value)
            })
        };
        if !valid_session(author_session_id)
            || !valid_session(reviewer_session_id)
            || author_session_id == reviewer_session_id
        {
            return Err(
                "reviewer and author must have distinct valid controller session identities".into(),
            );
        }
    }
    Ok(())
}
