use crate::core::prompt;
use crate::core::state::PlannerState;
use crate::harness::prompt_catalog::{self, PromptRole, RoleInputV1, RolePayloadV1};

pub(super) fn planner_conversation(
    state: &PlannerState,
    prompt_message: &str,
    turn_context: String,
    task_note: Option<&str>,
    operator_persona: String,
) -> Result<prompt_catalog::RenderedPrompt, String> {
    let workspace_uid = state.planning_store.project_id.hyphenated().to_string();
    let feature_uid = state
        .active_feature
        .as_ref()
        .and_then(|(_, markdown)| {
            crate::domain::ArtifactIdentity::from_markdown(markdown)
                .ok()
                .flatten()
                .map(|identity| identity.uid)
        })
        .unwrap_or_else(|| format!("planning:{workspace_uid}"));
    prompt_catalog::render(&RoleInputV1 {
        role: PromptRole::PlannerConversationV5,
        workspace_uid,
        feature_uid,
        task_uid: None,
        original_goal: crate::core::context_build::clip_bytes(prompt_message, 16 * 1024),
        approved_spec_ref: None,
        approved_spec_hash: None,
        decision_refs: vec![],
        evidence_manifest: vec![],
        execution_policy_ref: "planning-turn/application-validation-only/v1".into(),
        output_schema_version: PromptRole::PlannerConversationV5
            .output_schema_version()
            .into(),
        payload: RolePayloadV1::PlannerTurn {
            turn_context,
            workflow_instructions: task_note
                .is_none()
                .then(|| prompt::WORKFLOW_INSTRUCTIONS.to_owned()),
            operator_persona,
            task_mode_note: task_note.map(str::to_owned),
        },
    })
}
