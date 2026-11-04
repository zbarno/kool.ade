use super::*;

fn private_store(
    code: &std::path::Path,
    local: &std::path::Path,
) -> crate::artifacts::planning_store::PlanningStore {
    crate::artifacts::planning_store::PlanningStore::for_code_repository(
        crate::core::state::test_support::project_id_for_code_root(code),
        local,
        code,
    )
}

fn write_published_feature(
    code: &std::path::Path,
    feature_id: &str,
    markdown: &str,
    status: crate::domain::ChangeStatus,
) -> String {
    let body = crate::test_support::current_change_fixture(markdown, status).unwrap();
    let identity = crate::domain::ArtifactIdentity::from_markdown(&body)
        .unwrap()
        .unwrap();
    let feature_root = code.join("docs/koolade/features");
    std::fs::create_dir_all(&feature_root).unwrap();
    std::fs::write(feature_root.join(format!("{}.md", identity.uid)), &body).unwrap();
    let index_path = feature_root.join("AGENTS.md");
    let mut index =
        std::fs::read_to_string(&index_path).unwrap_or_else(|_| "# Navigation\n".into());
    index.push_str(&format!(
        "\n- [{feature_id}](./{}.md) - Synthetic published feature.\n",
        identity.uid
    ));
    std::fs::write(index_path, index).unwrap();
    body
}

fn commit_published_features(code: &std::path::Path) {
    for args in [
        ["add", "docs/koolade/features"].as_slice(),
        ["commit", "-qm", "Seed synthetic published features"].as_slice(),
    ] {
        assert!(
            std::process::Command::new("git")
                .args(args)
                .current_dir(code)
                .status()
                .unwrap()
                .success()
        );
    }
}

fn lines(n: usize) -> Vec<(String, String)> {
    (0..n)
        .map(|i| (format!("sp{i}"), format!("msg {i}")))
        .collect()
}

fn seed_root_repository_manifest(store: &crate::artifacts::planning_store::PlanningStore) {
    let manifest = crate::core::project_repos::ProjectManifest {
        repositories: vec![crate::core::project_repos::Repository {
            id: "root".into(),
            role: "Primary code".into(),
            remote: String::new(),
            display_name: None,
        }],
    };
    store
        .atomic_write(
            crate::artifacts::planning_store::paths::PROJECT_MANIFEST,
            &serde_json::to_vec_pretty(&manifest).unwrap(),
        )
        .unwrap();
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
    let local = root.join("private-local-state");
    std::fs::create_dir_all(&code).unwrap();
    std::fs::create_dir_all(&local).unwrap();
    crate::core::state::test_support::seed_published_product_document(&code).unwrap();
    let store = private_store(&code, &local);
    let manifest = crate::core::project_repos::ProjectManifest {
        repositories: vec![crate::core::project_repos::Repository {
            id: "root".into(),
            role: "Primary code".into(),
            remote: String::new(),
            display_name: None,
        }],
    };
    store
        .atomic_write(
            crate::artifacts::planning_store::paths::PROJECT_MANIFEST,
            &serde_json::to_vec_pretty(&manifest).unwrap(),
        )
        .unwrap();
    store
        .atomic_write(
            "planning/imports/reference.md",
            b"Shared-only evidence that the worker must receive.",
        )
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
    assert!(prompt.contains("LOCAL USER IDENTITY (config/project.md)"));
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
    let code = root.join("code");
    let local = root.join("private-local-state");
    std::fs::create_dir_all(&code).unwrap();
    std::fs::create_dir_all(&local).unwrap();
    crate::core::state::test_support::seed_published_product_document(&code).unwrap();
    write_published_feature(
        &code,
        "CHG-001",
        "# CHG-001: Active\n\n**Status:** Draft\n\n## Intent\n\nOne feature.\n\n## Current Behavior\n\nCurrent.\n\n## Desired Behavior\n\nDesired.\n\n## Scope\n\nOne area.\n\n## Affected Product Areas\n\n`product:current-capabilities`\n\n## Requirements\n\nOne.\n\n## Decisions and Assumptions\n\nNone.\n\n## Acceptance Criteria\n\nOne.\n",
        crate::domain::ChangeStatus::Draft,
    );
    for n in 2..=250 {
        write_published_feature(
            &code,
            &format!("CHG-{n:03}"),
            &format!(
                "# CHG-{n:03}: Historical {n}\n\n## Intent\n\nKeep historical test context.\n\n## Current Behavior\n\nHISTORICAL_SECRET_{n}\n\n## Desired Behavior\n\nRetain the documented behavior.\n\n## Scope\n\nHistorical fixture only.\n\n## Affected Product Areas\n\n`product:current-capabilities`\n\n## Requirements\n\n- Retain the recorded behavior.\n\n## Decisions and Assumptions\n\nNone.\n\n## Acceptance Criteria\n\n- Historical behavior remains searchable.\n"
            ),
            crate::domain::ChangeStatus::Implemented,
        );
    }
    commit_published_features(&code);
    let store = private_store(&code, &local);
    seed_root_repository_manifest(&store);
    let mut state = PlannerState::load_with_store(&code, &store).unwrap();
    let before_ctx = TurnContext::build(&state, "Plan this feature", &lines(2));
    assert!(before_ctx.selected_documents.is_empty());
    let before = crate::core::prompt::render_prompt(&before_ctx)
        + &crate::core::prompt::workflow_context(
            &state,
            crate::core::workflow::TurnPurpose::Interview,
        );
    std::fs::create_dir_all(local.join("planning/imports")).unwrap();
    for n in 2..=250 {
        std::fs::write(
            local
                .join("planning/imports")
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
                directory: format!("state/tasks/historical-{n}"),
                count: 1,
                created_at_ms: 0,
            });
        let mut item = crate::domain::OpenItem::new(
            format!("CLR-{n:03}"),
            crate::domain::Priority::Normal,
            crate::domain::ItemKind::Question,
            "General".into(),
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
    let code = root.join("code");
    let local = root.join("private-local-state");
    std::fs::create_dir_all(&code).unwrap();
    std::fs::create_dir_all(&local).unwrap();
    crate::core::state::test_support::seed_published_product_document(&code).unwrap();
    write_published_feature(
        &code,
        "CHG-001",
        "# CHG-001: Active\n\n**Status:** Draft\n\n## Intent\n\nDURABLE_DECISION_MARKER\n\n## Current Behavior\n\nCurrent.\n\n## Desired Behavior\n\nDesired.\n\n## Scope\n\nOne area.\n\n## Affected Product Areas\n\n`product:current-capabilities`\n\n## Requirements\n\nOne.\n\n## Decisions and Assumptions\n\nAccepted.\n\n## Acceptance Criteria\n\nOne.\n",
        crate::domain::ChangeStatus::Draft,
    );
    commit_published_features(&code);
    std::fs::create_dir_all(local.join("state/derived")).unwrap();
    std::fs::write(
        local.join("state/derived/summary.json"),
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
    let store = private_store(&code, &local);
    seed_root_repository_manifest(&store);
    let state = PlannerState::load_with_store(&code, &store).unwrap();
    let first = crate::core::prompt::render_prompt(&TurnContext::build(
        &state,
        "Continue the active feature",
        &conversations,
    ));
    assert!(first.contains("DURABLE_DECISION_MARKER"));
    assert!(!first.contains("EXPIRED_CHAT_MARKER"));
    assert!(!first.contains("DISPOSABLE_CACHE_MARKER"));
    std::fs::remove_dir_all(local.join("state/derived")).unwrap();
    let reloaded = PlannerState::load_with_store(&code, &store).unwrap();
    let rebuilt = crate::core::prompt::render_prompt(&TurnContext::build(
        &reloaded,
        "Continue the active feature",
        &[],
    ));
    assert!(rebuilt.contains("DURABLE_DECISION_MARKER"));
    assert!(!rebuilt.contains("DISPOSABLE_CACHE_MARKER"));
    let _ = std::fs::remove_dir_all(root);
}

#[test]
fn byte_clipping_includes_marker_and_preserves_unicode() {
    for text in ["a".repeat(20_000), "質問🦀".repeat(6_000)] {
        for cap in [0, 4, 16, 16_384] {
            let clipped = clip_bytes(&text, cap);
            assert!(clipped.len() <= cap);
            assert!(text.starts_with(clipped.split('\n').next().unwrap()) || cap < 18);
            if cap >= 18 {
                assert!(clipped.ends_with("\n[…truncated…]"));
            }
        }
    }
    let exact = "🦀".repeat(4096);
    assert_eq!(clip_bytes(&exact, 16_384), exact);
}
