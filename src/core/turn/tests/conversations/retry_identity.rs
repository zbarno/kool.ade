use super::*;
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};

struct ReplaceFeatureOnce {
    store: crate::artifacts::planning_store::PlanningStore,
    replacement: String,
    calls: Arc<AtomicUsize>,
}

impl AiHarness for ReplaceFeatureOnce {
    fn label(&self) -> String {
        "replace-feature-once".into()
    }

    fn check_available(&self) -> Result<String, AppError> {
        Ok(self.label())
    }

    fn execute(&self, _request: &PlanningRequest) -> Result<HarnessOutcome, AppError> {
        self.calls.fetch_add(1, Ordering::SeqCst);
        let changes = crate::core::apply::private_document_draft_changes(
            &self.store,
            &[("feature:F1".into(), self.replacement.clone())],
        )
        .map_err(|error| AppError::Other(error.to_string()))?;
        self.store
            .transaction(&changes, None)
            .map_err(|error| AppError::Other(error.to_string()))?;
        Ok(HarnessOutcome {
            final_text: serde_json::json!({
                "schema_version": 1,
                "assistant_message": "I recorded the saved planning answer.",
                "change_summary": "record the saved answer"
            })
            .to_string(),
            envelope: None,
            stderr_tail: String::new(),
        })
    }
}

fn feature(id: &str, title: &str) -> (String, String) {
    let identified = crate::domain::ArtifactIdentity::preserve_markdown(
        &format!("# {id}: {title}\n\n## Intent\n\n{title}\n\n## Current Behavior\n\nCurrent behavior is documented.\n\n## Desired Behavior\n\n{title} is supported.\n\n## Scope\n\nThis feature has a bounded scope.\n\n## Affected Product Areas\n\n`product:overview`\n\n## Requirements\n\n- {id}-R1 (MUST). Support {title}.\n\n## Decisions and Assumptions\n\n- No unresolved decisions.\n\n## Acceptance Criteria\n\n1. {title} is verified.\n"),
        None,
        id,
        title,
    )
    .unwrap();
    let identity = crate::domain::ArtifactIdentity::from_markdown(&identified)
        .unwrap()
        .unwrap();
    let markdown = crate::domain::ChangeMetadata::write_markdown(
        &identified,
        &identity,
        crate::domain::ChangeStatus::Draft,
    )
    .unwrap();
    (markdown, identity.uid)
}

#[test]
/// Production boundary: a real task reply turn must keep the original feature
/// identity across automatic revision retries. Validator tests cannot prove
/// the controller retains that pin; this uses temporary planning and chat
/// files with no Docker, SSH, or network and low setup cost.
fn task_reply_keeps_original_feature_uid_after_revision_retry() {
    let (mut inputs, root) = inputs_for("task_reply_retry_identity", "Continue planning F1.");
    let store = inputs.state.planning_store.clone();
    let (original, original_uid) = feature("F1", "Original feature");
    let (replacement, replacement_uid) = feature("F1", "Replacement feature");
    assert_ne!(original_uid, replacement_uid);
    let changes = crate::core::apply::private_document_draft_changes(
        &store,
        &[("feature:F1".into(), original)],
    )
    .unwrap();
    store.transaction(&changes, None).unwrap();
    inputs.state = PlannerState::load_with_store(&root, &store).unwrap();
    assert_eq!(
        crate::core::turn::execute::task_feature_uid(&inputs.state, "feature:F1").as_deref(),
        Some(original_uid.as_str())
    );

    let slug = format!("turn-retry-identity-{}", uuid::Uuid::new_v4());
    let mut chats = crate::persistence::task_chats::TaskChats::default();
    chats.ensure_loaded(&slug);
    let user = crate::domain::ChatMessage::new(
        crate::domain::ChatRole::User,
        "Keep the original plan direction.",
        Some("feature:F1".into()),
    );
    let user_id = user.id.clone();
    let claim_id = chats
        .append_user_reply_and_claim(&slug, "feature:F1", user)
        .unwrap();
    let calls = Arc::new(AtomicUsize::new(0));
    let controller = TurnController::start_task_reply(
        inputs,
        Box::new(ReplaceFeatureOnce {
            store: store.clone(),
            replacement,
            calls: calls.clone(),
        }),
        Some("feature:F1".into()),
        slug.clone(),
        user_id,
        Some(claim_id),
    );

    match drain(&controller) {
        TurnOutcome::Rejected { problems, .. } => assert!(
            problems.iter().any(|problem| {
                problem.starts_with("Planning task feature identity mismatch:")
                    && problem.contains(&original_uid)
                    && problem.contains(&replacement_uid)
            }),
            "retry did not reject the replaced feature identity: {problems:?}"
        ),
        other => panic!("replacement feature must not receive the saved reply: {other:?}"),
    }
    assert_eq!(calls.load(Ordering::SeqCst), 1);
    let messages = &chats.messages["feature:F1"];
    assert_eq!(
        messages
            .iter()
            .filter(|message| message.role == crate::domain::ChatRole::User)
            .count(),
        1
    );

    let _ = std::fs::remove_dir_all(crate::persistence::project_dir(&slug));
    let _ = std::fs::remove_dir_all(root);
}
