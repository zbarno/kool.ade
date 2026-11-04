use super::*;
use crate::harness::RetrievalPlan;
use std::sync::Mutex;

struct RetrievalModel {
    plan: RetrievalPlan,
    request: Mutex<Option<String>>,
}

impl AiHarness for RetrievalModel {
    fn label(&self) -> String {
        "retrieval fixture".into()
    }

    fn check_available(&self) -> Result<String, AppError> {
        Ok(self.label())
    }

    fn execute(
        &self,
        _request: &PlanningRequest,
    ) -> Result<crate::harness::HarnessOutcome, AppError> {
        panic!("retrieval planning must use the dedicated harness method")
    }

    fn plan_retrieval(&self, request: &PlanningRequest) -> Result<Option<RetrievalPlan>, AppError> {
        *self.request.lock().unwrap() = Some(request.prompt_body.clone());
        Ok(Some(self.plan.clone()))
    }
}

fn fixture() -> (PlannerState, std::path::PathBuf) {
    let root = std::env::temp_dir().join(format!(
        "koolade-context-retrieval-{}-{}",
        std::process::id(),
        uuid::Uuid::new_v4()
    ));
    std::fs::create_dir_all(&root).unwrap();
    assert!(
        std::process::Command::new("git")
            .args(["init", "-q", "-b", "main"])
            .current_dir(&root)
            .status()
            .unwrap()
            .success()
    );
    let docs = root.join("docs/koolade");
    let fixture_docs = [
        (
            "AGENTS.md",
            "# Navigation\n\n- [architecture/](./architecture/AGENTS.md) - Read for architecture.\n- [product/](./product/AGENTS.md) - Read for product.\n",
        ),
        (
            "architecture/AGENTS.md",
            "# Navigation\n\n- [architecture-and-constraints.md](./architecture-and-constraints.md) - Read for architecture constraints.\n- [quality-and-acceptance.md](./quality-and-acceptance.md) - Read for quality requirements.\n",
        ),
        (
            "architecture/architecture-and-constraints.md",
            "# Architecture and Constraints\n\nSession credentials are restored from the local session store.\n",
        ),
        (
            "architecture/quality-and-acceptance.md",
            "# Quality and Acceptance\n\nUNSELECTED_RISK_SOURCE\n",
        ),
        (
            "product/AGENTS.md",
            "# Navigation\n\n- [overview.md](./overview.md) - Read for product overview.\n- [offline-sync.md](./offline-sync.md) - Read for offline sync.\n",
        ),
        (
            "product/overview.md",
            "# Product Overview\n\nThe product restores sessions safely.\n",
        ),
        (
            "product/offline-sync.md",
            "# Offline Sync\n\nThe offline queue replays after connectivity returns.\n",
        ),
    ];
    for (relative, content) in fixture_docs {
        let path = docs.join(relative);
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(path, content).unwrap();
    }
    for (key, value) in [
        ("user.name", "Fixture User"),
        ("user.email", "fixture@example.test"),
    ] {
        assert!(
            std::process::Command::new("git")
                .args(["config", key, value])
                .current_dir(&root)
                .status()
                .unwrap()
                .success()
        );
    }
    assert!(
        std::process::Command::new("git")
            .args(["add", "docs/koolade"])
            .current_dir(&root)
            .status()
            .unwrap()
            .success()
    );
    assert!(
        std::process::Command::new("git")
            .args(["commit", "-qm", "publish fixture specifications"])
            .current_dir(&root)
            .status()
            .unwrap()
            .success()
    );
    // Load only after publishing fixture documents: the state pins the code
    // repository's current documentation revision and must never see later,
    // uncommitted document changes.
    let mut state = PlannerState::load(&root).unwrap();
    state.bootstrap_missing().unwrap();
    std::fs::create_dir_all(root.join("src/persistence")).unwrap();
    std::fs::write(
        root.join("src/persistence/session.rs"),
        "pub fn restore_session() { /* SESSION_RESTORE_IMPLEMENTATION */ }\n",
    )
    .unwrap();
    std::fs::create_dir_all(root.join("src/notifications")).unwrap();
    std::fs::write(
        root.join("src/notifications/push.rs"),
        "// UNSELECTED_PUSH_IMPLEMENTATION\n",
    )
    .unwrap();
    state.items = vec![
        crate::domain::OpenItem::new(
            "CLR-021".into(),
            crate::domain::Priority::High,
            crate::domain::ItemKind::Question,
            "Product".into(),
            "Should a restored session retain its pending request?".into(),
            "This determines safe retry behavior after restart.".into(),
        ),
        crate::domain::OpenItem::new(
            "CLR-022".into(),
            crate::domain::Priority::Normal,
            crate::domain::ItemKind::Question,
            "Product".into(),
            "Which notification sound should play?".into(),
            "This only affects notification preferences.".into(),
        ),
    ];
    (state, root)
}

fn select(model: &RetrievalModel, state: &PlannerState, message: &str) -> ContextSelection {
    let (progress, _rx) = std::sync::mpsc::channel();
    super::select(
        model,
        state,
        message,
        &[],
        Duration::from_secs(60),
        progress,
        Arc::new(AtomicBool::new(false)),
    )
    .unwrap()
    .expect("fixture returns a generated retrieval plan")
}

#[test]
fn model_selected_sources_follow_the_issue_without_literal_names_in_the_request() {
    let (state, root) = fixture();
    let model = RetrievalModel {
        plan: RetrievalPlan {
            documents: vec![
                "published:docs/koolade/architecture/architecture-and-constraints.md".into(),
            ],
            open_items: vec!["CLR-021".into()],
            repository_areas: vec!["src/persistence".into()],
        },
        request: Mutex::new(None),
    };
    let user_message = "The session comes back empty after I restart the application.";
    let selected = select(&model, &state, user_message);
    let ctx = crate::core::context_build::TurnContext::build_with_retrieval(
        &state,
        user_message,
        &[],
        Some(&selected),
    );
    let prompt = crate::core::prompt::render_prompt(&ctx);

    assert!(
        model
            .request
            .lock()
            .unwrap()
            .as_ref()
            .unwrap()
            .contains(user_message)
    );
    assert!(prompt.contains("Session credentials are restored from the local session store."));
    assert!(prompt.contains("Source: docs/koolade/architecture/architecture-and-constraints.md"));
    assert!(prompt.contains("CLR-021"));
    assert!(prompt.contains("This determines safe retry behavior after restart."));
    assert!(prompt.contains("repo:src/persistence/session.rs"));
    assert!(prompt.contains("SESSION_RESTORE_IMPLEMENTATION"));
    assert!(!prompt.contains("UNSELECTED_RISK_SOURCE"));
    assert!(!prompt.contains("UNSELECTED_PUSH_IMPLEMENTATION"));
    assert!(!prompt.contains("CLR-022"));
    let _ = std::fs::remove_dir_all(root);
}

#[test]
fn model_can_retrieve_a_project_specific_optional_product_module() {
    let (state, root) = fixture();

    let model = RetrievalModel {
        plan: RetrievalPlan {
            documents: vec!["published:docs/koolade/product/offline-sync.md".into()],
            open_items: vec![],
            repository_areas: vec![],
        },
        request: Mutex::new(None),
    };
    let selected = select(
        &model,
        &state,
        "What happens to queued edits when the connection returns?",
    );
    assert_eq!(selected.documents.len(), 1);
    assert_eq!(
        selected.documents[0].id,
        "published:docs/koolade/product/offline-sync.md"
    );
    assert!(
        selected.documents[0]
            .content
            .contains("replays after connectivity")
    );
    assert!(
        selected.documents[0]
            .source_path
            .ends_with("offline-sync.md")
    );
    let _ = std::fs::remove_dir_all(root);
}

#[test]
fn invalid_references_are_ignored_and_retrieval_limits_are_deterministic() {
    let (state, root) = fixture();
    let plan = RetrievalPlan {
        documents: vec![
            "../../outside/secret.md".into(),
            "published:docs/koolade/architecture/architecture-and-constraints.md".into(),
            "published:docs/koolade/architecture/architecture-and-constraints.md".into(),
            "published:docs/koolade/product/overview.md".into(),
            "published:docs/koolade/product/missing.md".into(),
            "published:docs/koolade/architecture/quality-and-acceptance.md".into(),
        ],
        open_items: vec!["../../outside".into(), "CLR-022".into(), "CLR-021".into()],
        repository_areas: vec!["../outside".into(), "src/persistence".into()],
    };
    let model = RetrievalModel {
        plan,
        request: Mutex::new(None),
    };
    let selected = select(&model, &state, "Where is the behavior implemented?");

    assert_eq!(selected.documents.len(), 2);
    assert_eq!(selected.open_items.len(), 2);
    assert_eq!(selected.repository_areas.len(), 1);
    assert!(
        selected
            .documents
            .iter()
            .all(|doc| doc.source_path.starts_with("docs/koolade/"))
    );
    assert!(
        selected
            .repository_areas
            .iter()
            .all(|area| area.path == "src/persistence")
    );
    assert!(selected.repository_areas[0].content.chars().count() <= 16_100);
    let _ = std::fs::remove_dir_all(root);
}

#[test]
fn active_change_context_keeps_current_targets_and_bounds_older_entries() {
    let (mut state, root) = fixture();
    state.active_feature = Some(("CHG-001".into(), "# CHG-001: Current\n".into()));
    state.active_features = (1..=120)
        .map(|number| {
            (
                format!("CHG-{number:03}"),
                format!("# CHG-{number:03}: Active change {number}\n"),
            )
        })
        .collect();

    let prompt = crate::core::prompt::workflow_context(
        &state,
        crate::core::workflow::TurnPurpose::Interview,
    );

    assert!(prompt.contains("change CHG-001;"));
    assert_eq!(prompt.matches("change CHG-").count(), 40);
    assert!(prompt.contains("80 additional active changes omitted"));
    assert!(!prompt.contains("change CHG-120;"));
    let _ = std::fs::remove_dir_all(root);
}

#[path = "tests/compare_plans.rs"]
mod compare_plans;
