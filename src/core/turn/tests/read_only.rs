//! Production turn pipeline boundary: clarification must never apply model edits.
//! A mocked harness is necessary to return an otherwise writable envelope through
//! the real worker; a validator-only test cannot establish the bypass. Requires
//! local Git and a temporary repository; no provider, network, Docker, or SSH.
//! Setup cost: one temporary Git repository and one local worker thread.
use super::*;

#[test]
fn clarification_keeps_repository_unchanged_even_when_the_model_returns_edits() {
    let (inputs, dir) = inputs_for("read_only_clarification", "What would this change do?");
    git_stdout(&dir, &["add", "-A"]);
    git_stdout(
        &dir,
        &[
            "commit",
            "--allow-empty",
            "-m",
            "Synthetic clarification baseline",
        ],
    );
    let before_head = git_stdout(&dir, &["rev-parse", "HEAD"]);
    let before_status = git_stdout(&dir, &["status", "--porcelain"]);
    let envelope: TurnEnvelope = serde_json::from_value(serde_json::json!({
        "schema_version": 2,
        "assistant_message": "It changes how the project is planned.",
        "change_summary": "This edit must never be applied",
        "document_updates": vision_update("An unauthorized replacement"),
        "open_items_added": [], "open_items_updated": [], "open_items_resolved": []
    }))
    .unwrap();
    let controller = TurnController::start_read_only(
        inputs,
        Box::new(ScriptedHarness {
            canned: Some(envelope),
            raw: None,
        }),
        None,
    );
    assert!(matches!(drain(&controller), TurnOutcome::Discussion { .. }));
    assert_eq!(git_stdout(&dir, &["rev-parse", "HEAD"]), before_head);
    assert_eq!(git_stdout(&dir, &["status", "--porcelain"]), before_status);
    let _ = std::fs::remove_dir_all(dir);
}

struct WorkspaceHarness {
    task_id: String,
    sha: String,
}
impl AiHarness for WorkspaceHarness {
    fn label(&self) -> String {
        "workspace-fixture".into()
    }
    fn check_available(&self) -> Result<String, AppError> {
        Ok(self.label())
    }
    fn execute(&self, request: &PlanningRequest) -> Result<HarnessOutcome, AppError> {
        let workspace = request
            .execution_workspace
            .as_ref()
            .expect("selected task receives read-only repository tools");
        workspace.validate().unwrap();
        assert_eq!(request.mode, crate::harness::ExecutionMode::Planning);
        assert_eq!(
            request.mode.tool_access(),
            crate::harness::ToolAccess::ReadOnly
        );
        assert_eq!(workspace.task_id, self.task_id);
        assert_eq!(request.task_id.as_deref(), Some(self.task_id.as_str()));
        assert_eq!(workspace.source_sha, self.sha);
        Ok(HarnessOutcome {
            final_text: "Repository explanation".into(),
            envelope: None,
            stderr_tail: String::new(),
        })
    }
}

/// Production boundary: the turn pipeline must carry the selected task's stable
/// identity and source HEAD into the application-owned read-only tool boundary.
/// A helper test cannot prove request preparation retains that binding. Mock
/// provider plus temporary Git only; no Docker/network/SSH; low setup cost.
#[test]
fn planning_task_investigation_binds_its_source_workspace() {
    let (inputs, dir) = inputs_for(
        "planning_workspace",
        "Explain the repository implementation",
    );
    git_stdout(&dir, &["add", "-A"]);
    git_stdout(
        &dir,
        &["commit", "--allow-empty", "-m", "Synthetic source baseline"],
    );
    let sha = git_stdout(&dir, &["rev-parse", "HEAD"]).trim().to_owned();
    let mut work = crate::core::planning_work::Work::new(
        "planning:source".into(),
        "Inspect source".into(),
        "Explain implementation".into(),
        "Inspecting".into(),
    );
    work.kind = crate::core::planning_work::WorkKind::Question;
    crate::core::planning_work::save(&inputs.state.planning_store, &[work.clone()]).unwrap();
    let controller = TurnController::start_read_only(
        inputs,
        Box::new(WorkspaceHarness {
            task_id: work.uid,
            sha,
        }),
        Some(work.key),
    );
    assert!(matches!(drain(&controller), TurnOutcome::Discussion { .. }));
    std::fs::remove_dir_all(dir).unwrap();
}

struct ContextOnlyHarness;
impl AiHarness for ContextOnlyHarness {
    fn label(&self) -> String {
        "context-only-fixture".into()
    }
    fn check_available(&self) -> Result<String, AppError> {
        Ok(self.label())
    }
    fn execute(&self, request: &PlanningRequest) -> Result<HarnessOutcome, AppError> {
        assert!(
            request.execution_workspace.is_none(),
            "a human choice discussion must not require Docker or repository tools"
        );
        assert!(request.task_id.is_none());
        Ok(HarnessOutcome {
            final_text: "Choice explanation".into(),
            envelope: None,
            stderr_tail: String::new(),
        })
    }
}

/// Same turn/request boundary as the investigation test above; a contextual
/// Human choice discussion remains tool-free even with a valid source HEAD.
#[test]
fn human_choice_discussion_remains_context_only_in_a_git_workspace() {
    let (mut inputs, dir) = inputs_for("context_only_choice", "Explain these choices");
    git_stdout(&dir, &["add", "-A"]);
    git_stdout(
        &dir,
        &[
            "commit",
            "--allow-empty",
            "-m",
            "Synthetic context baseline",
        ],
    );
    let item = crate::domain::OpenItem::new(
        "CLR-001".into(),
        crate::domain::Priority::High,
        crate::domain::ItemKind::Question,
        "General".into(),
        None,
        "Which storage?".into(),
        "Choose a product direction".into(),
    );
    let key = item.conversation_key().to_owned();
    inputs.state.items.push(item);
    let controller =
        TurnController::start_read_only(inputs, Box::new(ContextOnlyHarness), Some(key));
    assert!(matches!(drain(&controller), TurnOutcome::Discussion { .. }));
    std::fs::remove_dir_all(dir).unwrap();
}
