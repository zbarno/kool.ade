use super::*;

fn lines(n: usize) -> Vec<(String, String)> {
    (0..n)
        .map(|i| (format!("sp{i}"), format!("msg {i}")))
        .collect()
}

#[test]
fn conversation_tail_bounded_and_ordered() {
    let convos: Vec<ConversationLine> = TailPreview::preview(&lines(10));
    assert_eq!(convos.len(), 6);
    assert_eq!(convos.first().unwrap().speaker, "sp4");
    assert_eq!(convos.last().unwrap().speaker, "sp9");
}

#[allow(dead_code)]
struct TailPreview;
impl TailPreview {
    fn preview(recent: &[(String, String)]) -> Vec<ConversationLine> {
        recent
            .iter()
            .rev()
            .take(CONVERSATION_TAIL_MESSAGES)
            .collect::<Vec<_>>()
            .iter()
            .rev()
            .map(|(s, t)| ConversationLine {
                speaker: s.clone(),
                text: clip(t, MESSAGE_CLIP_CHARS),
            })
            .collect()
    }
}

#[test]
fn clips_long_messages() {
    let s = "z".repeat(5000);
    let c = clip(&s, 1200);
    assert!(c.chars().count() <= 1200);
    assert!(c.ends_with("[…truncated…]"));
}

#[test]
fn managed_imports_are_loaded_from_the_planning_store_into_turn_context() {
    let root = std::env::temp_dir().join(format!(
        "koolade_managed_import_context_{}-{}",
        std::process::id(),
        uuid::Uuid::new_v4()
    ));
    let code = root.join("code");
    let planning = root.join("planning");
    std::fs::create_dir_all(&code).unwrap();
    std::fs::create_dir_all(&planning).unwrap();
    for repo in [&code, &planning] {
        assert!(
            std::process::Command::new("git")
                .args(["init", "-q"])
                .current_dir(repo)
                .status()
                .unwrap()
                .success()
        );
    }
    let store = crate::artifacts::planning_store::PlanningStore::new(
        uuid::Uuid::new_v4(),
        &planning,
        crate::artifacts::planning_store::StoreMode::ManagedLocal,
    );
    store
        .atomic_write(
            "planning/imports/reference.md",
            b"Shared-only evidence that the worker must receive.",
        )
        .unwrap();
    store
        .atomic_write("planning/product/index.md", b"# Managed product index\n")
        .unwrap();
    let state = PlannerState::load_with_store(&code, &store).unwrap();

    let context = TurnContext::build(&state, "Use the imported reference", &[]);
    assert_eq!(context.imports.len(), 1);
    assert_eq!(
        context.imports[0].content.as_deref(),
        Some("Shared-only evidence that the worker must receive.")
    );
    assert!(
        crate::core::prompt::render_prompt(&context)
            .contains("Shared-only evidence that the worker must receive.")
    );
    let prompt = crate::core::prompt::render_prompt(&context);
    assert!(prompt.contains("Other product modules are under planning/product/"));
    assert!(prompt.contains("STAKEHOLDERS & OWNERSHIP (config/project.md)"));
    assert!(!prompt.contains(".koolade-packet/planning/product/"));
    assert!(!code.join("planning/imports/reference.md").exists());
    let _ = std::fs::remove_dir_all(root);
}

#[test]
fn historical_features_tasks_and_items_do_not_expand_normal_prompt_without_bound() {
    let root = std::env::temp_dir().join(format!(
        "koolade_context_growth_{}-{}",
        std::process::id(),
        chrono::Utc::now().timestamp_nanos_opt().unwrap()
    ));
    std::fs::create_dir_all(&root).unwrap();
    assert!(
        std::process::Command::new("git")
            .args(["init", "-q"])
            .current_dir(&root)
            .status()
            .unwrap()
            .success()
    );
    let legacy = crate::artifacts::spec_doc::bootstrap_template("Demo");
    crate::artifacts::product_docs::migrate(&root, &legacy).unwrap();
    let active = root.join(".koolade-packet/planning/changes/CHG-001-active/specification.md");
    std::fs::create_dir_all(active.parent().unwrap()).unwrap();
    std::fs::write(active,
            "# CHG-001: Active\n\n**Status:** Draft\n\n## Intent\n\nOne feature.\n\n## Current Behavior\n\nCurrent.\n\n## Desired Behavior\n\nDesired.\n\n## Scope\n\nOne area.\n\n## Affected Product Areas\n\n`current-capabilities.md`\n\n## Requirements\n\nOne.\n\n## Decisions and Assumptions\n\nNone.\n\n## Acceptance Criteria\n\nOne.\n").unwrap();
    let mut state = PlannerState::load(&root).unwrap();
    let before_ctx = TurnContext::build(&state, "Plan this feature", &lines(2));
    assert!(before_ctx.selected_documents.is_empty());
    let before = crate::core::prompt::render_prompt(&before_ctx)
        + &crate::core::prompt::workflow_context(
            &state,
            crate::core::workflow::TurnPurpose::Interview,
        );
    std::fs::create_dir_all(root.join(".koolade-packet/planning/imports")).unwrap();
    for n in 2..=250 {
        let name = format!("CHG-{n:03}-historical");
        let dir = root.join(".koolade-packet/planning/changes").join(name);
        std::fs::create_dir(&dir).unwrap();
        std::fs::write(
            dir.join("specification.md"),
            format!("# Historical {n}\n\n**Status:** Implemented\n\nHISTORICAL_SECRET_{n}\n"),
        )
        .unwrap();
        std::fs::write(
            root.join(".koolade-packet/planning/imports")
                .join(format!("source-{n:03}.txt")),
            "evidence",
        )
        .unwrap();
        state
            .workflow
            .task_batches
            .push(crate::core::workflow::TaskBatchRef {
                identity: None,
                feature: format!("Historical batch {n}"),
                directory: format!(".koolade-packet/planning/tasks/historical-{n}"),
                count: 1,
                created_at_ms: 0,
            });
        let mut item = crate::domain::OpenItem::new(
            format!("CLR-{n:03}"),
            crate::domain::Priority::Normal,
            crate::domain::ItemKind::Question,
            "General".into(),
            Some("All".into()),
            format!("Historical item {n}?"),
            "Long context ".repeat(100),
        );
        item.authority = crate::domain::Authority::Human;
        state.items.push(item);
    }
    let ctx = TurnContext::build(&state, "Plan this feature", &lines(1000));
    let after = crate::core::prompt::render_prompt(&ctx)
        + &crate::core::prompt::workflow_context(
            &state,
            crate::core::workflow::TurnPurpose::Interview,
        );
    assert!(
        after.len() < before.len() + 20_000,
        "prompt grew from {} to {}",
        before.len(),
        after.len()
    );
    assert!(!after.contains("HISTORICAL_SECRET_250"));
    assert!(ctx.conversation.len() <= 6);
    assert!(ctx.imports.len() <= 30);
    assert!(ctx.selected_documents.len() <= 5);
    let _ = std::fs::remove_dir_all(root);
}

#[test]
fn accepted_feature_knowledge_survives_chat_expiration_and_derived_cache_deletion() {
    let root = std::env::temp_dir().join(format!(
        "koolade_context_authority_{}-{}",
        std::process::id(),
        chrono::Utc::now().timestamp_nanos_opt().unwrap()
    ));
    std::fs::create_dir_all(&root).unwrap();
    std::fs::create_dir_all(root.join(".koolade-packet/state/derived")).unwrap();
    let legacy = crate::artifacts::spec_doc::bootstrap_template("Demo");
    crate::artifacts::product_docs::migrate(&root, &legacy).unwrap();
    let active = root.join(".koolade-packet/planning/changes/CHG-001-active/specification.md");
    std::fs::create_dir_all(active.parent().unwrap()).unwrap();
    std::fs::write(active,
            "# CHG-001: Active\n\n**Status:** Draft\n\n## Intent\n\nDURABLE_DECISION_MARKER\n\n## Current Behavior\n\nCurrent.\n\n## Desired Behavior\n\nDesired.\n\n## Scope\n\nOne area.\n\n## Affected Product Areas\n\n`product:current-capabilities`\n\n## Requirements\n\nOne.\n\n## Decisions and Assumptions\n\nAccepted.\n\n## Acceptance Criteria\n\nOne.\n").unwrap();
    std::fs::write(
        root.join(".koolade-packet/state/derived/summary.json"),
        "DISPOSABLE_CACHE_MARKER",
    )
    .unwrap();
    let conversations = (0..100)
        .map(|n| {
            (
                "user".to_string(),
                if n == 0 {
                    "EXPIRED_CHAT_MARKER".into()
                } else {
                    format!("recent {n}")
                },
            )
        })
        .collect::<Vec<_>>();
    let state = PlannerState::load(&root).unwrap();
    let first = crate::core::prompt::render_prompt(&TurnContext::build(
        &state,
        "Continue the active feature",
        &conversations,
    ));
    assert!(first.contains("DURABLE_DECISION_MARKER"));
    assert!(!first.contains("EXPIRED_CHAT_MARKER"));
    assert!(!first.contains("DISPOSABLE_CACHE_MARKER"));
    std::fs::remove_dir_all(root.join(".koolade-packet/state/derived")).unwrap();
    let reloaded = PlannerState::load(&root).unwrap();
    let rebuilt = crate::core::prompt::render_prompt(&TurnContext::build(
        &reloaded,
        "Continue the active feature",
        &[],
    ));
    assert!(rebuilt.contains("DURABLE_DECISION_MARKER"));
    assert!(!rebuilt.contains("DISPOSABLE_CACHE_MARKER"));
    let _ = std::fs::remove_dir_all(root);
}
