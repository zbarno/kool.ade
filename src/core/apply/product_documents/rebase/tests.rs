use super::*;

fn git(repo: &std::path::Path, args: &[&str]) {
    let status = std::process::Command::new("git")
        .args(args)
        .current_dir(repo)
        .status()
        .unwrap();
    assert!(status.success());
}

/// Production boundary: exact Git objects -> private draft rebasing -> state
/// projection. Unit validators cannot prove pin advancement or identity retention.
/// Disposable Git/local files only, no providers or Docker; low setup cost.
fn fixture() -> (
    tempfile::TempDir,
    std::path::PathBuf,
    crate::artifacts::planning_store::PlanningStore,
    String,
    LocalSpecDraftV1,
) {
    let dir = tempfile::tempdir().unwrap();
    let code = dir.path().join("code");
    crate::core::state::test_support::seed_published_product_document(&code).unwrap();
    let store = crate::artifacts::planning_store::PlanningStore::for_code_repository(
        uuid::Uuid::new_v4(),
        dir.path().join("private"),
        &code,
    );
    let initial = PlannerState::load_with_store(&code, &store).unwrap();
    let text = "# F1: Restore planning\n\n## Intent\nKeep intent.\n\n## Current Behavior\nDraft hidden.\n\n## Desired Behavior\nKeep planning.\n\n## Scope\nLocal recovery.\n\n## Affected Product Areas\nPlanning.\n\n## Requirements\nPreserve identity.\n\n## Decisions and Assumptions\nNo new decisions.\n\n## Acceptance Criteria\nSame UID survives.\n";
    let original =
        crate::test_support::current_change_fixture(text, crate::domain::ChangeStatus::Draft)
            .unwrap();
    let uid = crate::domain::ArtifactIdentity::from_markdown(&original)
        .unwrap()
        .unwrap()
        .uid;
    let files = changes(&initial.planning_store, &[("feature:F1".into(), original)]).unwrap();
    let writes = files
        .iter()
        .map(|(p, s)| (p.clone(), s.as_bytes().to_vec()))
        .collect::<Vec<_>>();
    initial
        .planning_store
        .transaction(&writes, Some(&initial.baseline_planning_revision))
        .unwrap();
    let old = load_current(&initial.planning_store).unwrap().unwrap();
    (dir, code, store, uid, old)
}

#[test]
fn unrelated_published_changes_recover_the_original_private_feature() {
    let (_dir, code, store, uid, old) = fixture();
    std::fs::write(code.join("README.md"), "Unrelated code change").unwrap();
    git(&code, &["add", "README.md"]);
    git(
        &code,
        &[
            "-c",
            "user.name=Test",
            "-c",
            "user.email=test@example.invalid",
            "commit",
            "-qm",
            "advance code",
        ],
    );
    let missing = PlannerState::load_with_store(&code, &store).unwrap();
    assert!(missing.active_features.is_empty());
    let mut pending = crate::core::planning_work::Work::new(
        "task:pending".into(),
        "Original request".into(),
        "Keep planning".into(),
        String::new(),
    );
    pending.feature_uid = Some(uid.clone());
    let cards = crate::core::planning_work::cards(&missing, &[pending]);
    assert_eq!(
        cards[0].status,
        crate::core::planning_work::WorkStatus::NeedsAttention
    );
    assert!(cards[0].detail.contains("older published revision"));
    let restored = super::super::restore_missing_feature(&missing, &uid).unwrap();
    assert_eq!(restored.active_features.len(), 1);
    let current = restored.local_spec_draft.unwrap();
    assert_eq!(current.changes, old.changes);
    assert_ne!(current.proposal_sha256, old.proposal_sha256);
    assert!(
        store
            .root
            .join(format!("state/spec_drafts/{}.json", old.proposal_sha256))
            .exists()
    );
    assert_eq!(current.base_published_spec_pin, restored.published_spec_pin);
    assert!(restored.workflow.approved_features.is_empty());
}

fn advance(code: &std::path::Path) {
    std::fs::write(code.join("README.md"), "Unrelated source change").unwrap();
    git(code, &["add", "."]);
    git(
        code,
        &[
            "-c",
            "user.name=Test",
            "-c",
            "user.email=test@example.invalid",
            "commit",
            "-qm",
            "advance published context",
        ],
    );
}

#[test]
fn changed_published_feature_content_requires_reconciliation_without_writes() {
    let (_dir, code, store, uid, old) = fixture();
    let path = code.join(format!("docs/koolade/features/{uid}.md"));
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(
        path,
        format!(
            "{}\nPublished requirements changed.\n",
            old.changes[0].content
        ),
    )
    .unwrap();
    advance(&code);
    let state = PlannerState::load_with_store(&code, &store).unwrap();
    let before = store.revision().unwrap();
    assert!(
        recover_current(&state, &uid)
            .unwrap_err()
            .to_string()
            .contains("documents changed")
    );
    assert_eq!(store.revision().unwrap(), before);
    assert_eq!(load_current(&store).unwrap().unwrap(), old);
}

#[test]
fn cancelled_drafts_and_stale_final_writes_remain_blocked() {
    for cancel in [false, true] {
        let (_dir, code, store, uid, old) = fixture();
        advance(&code);
        let mut state = PlannerState::load_with_store(&code, &store).unwrap();
        if cancel {
            crate::persistence::cancelled_work::save_expected(
                &store,
                &std::collections::BTreeSet::from([
                    crate::persistence::cancelled_work::planning_id(&uid),
                ]),
                &state.baseline_planning_revision,
            )
            .unwrap();
            state = PlannerState::load_with_store(&code, &store).unwrap();
        } else {
            store
                .atomic_write("state/reconciliation-marker.json", b"{}")
                .unwrap();
        }
        let before = store.revision().unwrap();
        assert!(recover_current(&state, &uid).is_err());
        assert_eq!(store.revision().unwrap(), before);
        assert_eq!(load_current(&store).unwrap().unwrap(), old);
    }
}

#[test]
fn changed_product_documents_block_a_product_draft() {
    let (_dir, code, store, uid, _) = fixture();
    let pinned = PlannerState::load_with_store(&code, &store)
        .unwrap()
        .planning_store;
    let files = changes(
        &pinned,
        &[(
            "product:overview".into(),
            "# Overview\n\nPrivate proposal.\n".into(),
        )],
    )
    .unwrap();
    let writes = files
        .into_iter()
        .map(|(p, s)| (p, s.into_bytes()))
        .collect::<Vec<_>>();
    pinned
        .transaction(&writes, Some(&pinned.revision().unwrap()))
        .unwrap();
    let path = code.join("docs/koolade/product/overview.md");
    let prior = std::fs::read_to_string(&path).unwrap();
    std::fs::write(path, format!("{prior}\nPublished scope changed.\n")).unwrap();
    advance(&code);
    let state = PlannerState::load_with_store(&code, &store).unwrap();
    let before = store.revision().unwrap();
    assert!(
        recover_current(&state, &uid)
            .unwrap_err()
            .to_string()
            .contains("documents changed")
    );
    assert_eq!(store.revision().unwrap(), before);
}

#[test]
fn replacement_refs_cannot_fabricate_published_ancestry() {
    let (_dir, code, store, uid, old) = fixture();
    let base = old
        .base_published_spec_pin
        .as_ref()
        .unwrap()
        .commit_sha
        .clone();
    git(&code, &["checkout", "--orphan", "disconnected"]);
    git(
        &code,
        &[
            "-c",
            "user.name=Test",
            "-c",
            "user.email=test@example.invalid",
            "commit",
            "-qm",
            "unrelated root",
        ],
    );
    let output = |args: &[&str]| {
        let out = std::process::Command::new("git")
            .args(args)
            .current_dir(&code)
            .output()
            .unwrap();
        assert!(out.status.success());
        String::from_utf8(out.stdout).unwrap().trim().to_owned()
    };
    let current = output(&["rev-parse", "HEAD"]);
    let tree = output(&["rev-parse", "HEAD^{tree}"]);
    let pretend_child = output(&[
        "-c",
        "user.name=Test",
        "-c",
        "user.email=test@example.invalid",
        "commit-tree",
        &tree,
        "-p",
        &base,
        "-m",
        "replacement child",
    ]);
    git(&code, &["update-ref", "refs/heads/main", &current]);
    git(&code, &["replace", &current, &pretend_child]);
    let state = PlannerState::load_with_store(&code, &store).unwrap();
    let before = store.revision().unwrap();
    assert!(recover_current(&state, &uid).is_err());
    assert_eq!(store.revision().unwrap(), before);
}
