use super::super::board_tests::{click_text, fixture, frame, text_position};
use super::*;
use std::sync::{Arc, Mutex};

struct Harness {
    replies: Mutex<std::collections::VecDeque<String>>,
    calls: Arc<Mutex<Vec<String>>>,
}
impl crate::harness::AiHarness for Harness {
    fn label(&self) -> String {
        "approval fixture".into()
    }
    fn check_available(&self) -> Result<String, crate::error::AppError> {
        Ok(self.label())
    }
    fn execute(
        &self,
        req: &crate::harness::PlanningRequest,
    ) -> Result<crate::harness::HarnessOutcome, crate::error::AppError> {
        self.calls.lock().unwrap().push(req.prompt_body.clone());
        let text = self.replies.lock().unwrap().pop_front().unwrap_or_else(|| {
            let tail = &req.prompt_body[req.prompt_body.len().saturating_sub(1200)..];
            panic!(
                "unexpected model call in {:?}; prompt tail:\n{tail}",
                req.mode
            )
        });
        Ok(crate::harness::HarnessOutcome {
            final_text: text,
            envelope: None,
            stderr_tail: String::new(),
        })
    }
}
fn harness(replies: Vec<String>) -> (Box<dyn crate::harness::AiHarness>, Arc<Mutex<Vec<String>>>) {
    let calls = Arc::new(Mutex::new(Vec::new()));
    (
        Box::new(Harness {
            replies: Mutex::new(replies.into()),
            calls: calls.clone(),
        }),
        calls,
    )
}
fn setup() -> (KooladeApp, std::path::PathBuf, serde_json::Value) {
    let mut app = fixture();
    let root = std::env::temp_dir().join(format!(
        "koolade-approval-{}",
        chrono::Utc::now().timestamp_nanos_opt().unwrap()
    ));
    std::fs::create_dir_all(&root).unwrap();
    for args in [
        vec!["init", "-q"],
        vec!["config", "user.name", "Fixture"],
        vec!["config", "user.email", "fixture@example.test"],
    ] {
        assert!(
            std::process::Command::new("git")
                .args(args)
                .current_dir(&root)
                .status()
                .unwrap()
                .success()
        );
    }
    let mut state = crate::core::state::PlannerState::load(&root).unwrap();
    state.bootstrap_missing().unwrap();
    let dir = root.join(".koolade-packet/planning/changes/CHG-004-saved-searches");
    std::fs::create_dir_all(&dir).unwrap();
    let mut spec = "# CHG-004: Saved searches\n\n**Status:** Ready\n".to_string();
    for heading in [
        "Intent",
        "Current Behavior",
        "Desired Behavior",
        "Scope",
        "Affected Product Areas",
        "Requirements",
        "Decisions and Assumptions",
        "Acceptance Criteria",
    ] {
        spec.push_str(&format!(
            "\n## {heading}\n\nPersist saved searches and restore them after restarting.\n"
        ));
    }
    spec = spec.replace(
        "## Affected Product Areas\n",
        "## Affected Product Areas\n\n`product:current-capabilities`\n",
    );
    let spec_path = dir.join("specification.md");
    spec = crate::artifacts::product_docs::identity::preserve_feature_identity(
        &spec_path, "CHG-004", &spec,
    )
    .unwrap();
    let identity = crate::domain::ArtifactIdentity::from_markdown(&spec)
        .unwrap()
        .unwrap();
    spec = crate::domain::ChangeMetadata::write_markdown(
        &spec,
        &identity,
        crate::domain::ChangeStatus::Ready,
    )
    .unwrap();
    std::fs::write(spec_path, spec).unwrap();
    let mut review: serde_json::Value = serde_json::from_str(include_str!(
        "../../../../tests/fixtures/interview-ready.json"
    ))
    .unwrap();
    review["updated_specification"] = serde_json::Value::Null;
    review["document_updates"] = serde_json::Value::Null;
    review["interview"]["feature_name"] = "Saved searches (CHG-004)".into();
    state.workflow.brief = Some(serde_json::from_value(review["interview"].clone()).unwrap());
    state.workflow.reviewed_specification = Some("Previous CHG-003 specification".into());
    crate::artifacts::task_docs::save_workflow(&root, &state.workflow).unwrap();
    let mut item = OpenItem::new(
        "CLR-026".into(),
        crate::domain::Priority::Normal,
        crate::domain::ItemKind::Question,
        "General".into(),
        None,
        "Branch retention choice".into(),
        "Settled".into(),
    );
    item.feature_id = Some("CHG-004".into());
    item.status = crate::domain::ItemStatus::Resolved;
    std::fs::write(
        root.join(".koolade-packet/planning/resolved-items.json"),
        serde_json::to_vec(&vec![item]).unwrap(),
    )
    .unwrap();
    crate::core::gitops::commit(
        &root,
        "fixture",
        &[
            ".koolade-packet/planning".into(),
            ".koolade-packet/config".into(),
        ],
    )
    .unwrap();
    let Screen::Connected(p) = &mut app.screen else {
        panic!()
    };
    p.state = crate::core::state::PlannerState::load(&root).unwrap();
    p.task_documents.clear();
    p.implementation_states.clear();
    p.chat_slug = root.join("runtime").to_string_lossy().into_owned();
    p.task_chats.remember_response(
        &p.chat_slug,
        "CLR-026",
        vec![ChatMessage::new(
            ChatRole::Agent,
            "Click Approve feature for implementation for CHG-004.",
            None,
        )],
    );
    (app, root, review)
}
fn finish(app: &mut KooladeApp) -> bool {
    let Screen::Connected(p) = &app.screen else {
        panic!()
    };
    let started = Instant::now();
    let outcome = loop {
        assert!(started.elapsed() < Duration::from_secs(15));
        if let Some(TurnEvt::Done(outcome)) = p
            .active_turn
            .as_ref()
            .expect("turn started")
            .poll(Duration::from_millis(20))
        {
            break *outcome;
        }
    };
    let applied = matches!(&outcome, TurnOutcome::Applied { .. });
    let Screen::Connected(mut p) = std::mem::replace(&mut app.screen, Screen::Welcome) else {
        panic!()
    };
    let _ = app.adopt_turn(&mut p, outcome);
    app.screen = Screen::Connected(p);
    applied
}

#[path = "tests/approval_flow.rs"]
mod approval_flow;
#[path = "tests/contract.rs"]
mod contract;
#[path = "tests/live_generation.rs"]
mod live_generation;
