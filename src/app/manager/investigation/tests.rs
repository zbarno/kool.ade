use super::approved_comparison_pins;

#[test]
/// Production boundary: a real broker receipt is verified by the Manager handoff,
/// converted to a baseline evidence record, and accepted only at its current
/// repository revision with a matching excerpt hash in a selected secondary
/// Python repository. Validator and broker unit tests alone cannot prove their
/// composition or checkout routing. Uses local Git only, no Docker,
/// network, or SSH; low setup cost.
fn verified_manager_broker_excerpt_passes_baseline_provenance_and_freshness_validation() {
    use crate::core::{
        investigation_broker::{
            InvestigationAuthorizationV1, InvestigationBroker, InvestigationBudgetV1,
            InvestigationContext, InvestigationEvidenceEnvelopeV1, InvestigationRequestV1,
            InvestigationScopeV1,
        },
        repository_knowledge::{EvidenceKind, ProfileRequest, scan_profile},
    };
    use sha2::Digest;
    use std::{path::PathBuf, process::Command, sync::atomic::AtomicBool};

    let root = std::env::temp_dir().join(format!(
        "koolade-baseline-evidence-{}",
        uuid::Uuid::new_v4()
    ));
    std::fs::create_dir_all(&root).unwrap();
    struct Cleanup(PathBuf);
    impl Drop for Cleanup {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }
    let _cleanup = Cleanup(root.clone());
    let primary = root.join("primary");
    std::fs::create_dir_all(&primary).unwrap();
    let git_at = |repo: &PathBuf, args: &[&str]| {
        let output = Command::new("git")
            .arg("-C")
            .arg(repo)
            .args(args)
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
        String::from_utf8_lossy(&output.stdout).trim().to_owned()
    };
    git_at(&primary, &["init", "-q"]);
    git_at(&primary, &["config", "user.name", "Synthetic Test"]);
    git_at(
        &primary,
        &["config", "user.email", "synthetic@example.invalid"],
    );
    std::fs::write(
        primary.join("README.md"),
        "Synthetic multi-repository project.\n",
    )
    .unwrap();
    git_at(&primary, &["add", "."]);
    git_at(&primary, &["commit", "-qm", "synthetic primary project"]);
    git_at(&primary, &["branch", "-M", "main"]);
    let repo = root.join("api");
    let cloned = Command::new("git")
        .args(["clone", "--quiet"])
        .arg(&primary)
        .arg(&repo)
        .output()
        .unwrap();
    assert!(
        cloned.status.success(),
        "{}",
        String::from_utf8_lossy(&cloned.stderr)
    );
    git_at(&repo, &["config", "user.name", "Synthetic Test"]);
    git_at(
        &repo,
        &["config", "user.email", "synthetic@example.invalid"],
    );
    git_at(
        &repo,
        &[
            "remote",
            "set-url",
            "origin",
            "https://example.invalid/synthetic/api.git",
        ],
    );
    std::fs::write(repo.join("app.py"), "def answer():\n    return 42\n").unwrap();
    git_at(&repo, &["add", "."]);
    git_at(&repo, &["commit", "-qm", "synthetic Python API"]);
    git_at(&repo, &["branch", "-M", "main"]);

    let project_id = uuid::Uuid::new_v4();
    let task_id = uuid::Uuid::new_v4().hyphenated().to_string();
    let store = crate::artifacts::planning_store::PlanningStore::new(
        project_id,
        root.join("planning"),
        crate::artifacts::planning_store::StoreMode::ManagedLocal,
    );
    let repositories = crate::core::project_repos::ProjectManifest {
        repositories: vec![
            crate::core::project_repos::Repository {
                id: "root".into(),
                role: "Primary code".into(),
                remote: String::new(),
                display_name: None,
            },
            crate::core::project_repos::Repository {
                id: "api".into(),
                role: "Python API".into(),
                remote: "https://example.invalid/synthetic/api.git".into(),
                display_name: None,
            },
        ],
    };
    crate::core::project_repos::connect_checkout(
        &store,
        &repositories,
        &primary,
        "api",
        &repo,
        &store.revision().unwrap(),
    )
    .unwrap();
    let source_ref = "refs/heads/main".to_owned();
    let profile = scan_profile(ProfileRequest {
        repository_id: "api",
        source_ref: &source_ref,
        code_root: &primary,
        planning_store: &store,
        repositories: &repositories,
        now_unix_ms: 1,
    })
    .unwrap();
    assert!(
        profile
            .languages
            .iter()
            .any(|language| language == "Python")
    );
    let source_sha = profile.key.commit_sha.clone();
    let workspace_id = format!("{}:{task_id}:{source_sha}", project_id.hyphenated());
    let task_revision = store.revision().unwrap();
    let context = InvestigationContext {
        source: crate::core::repository_knowledge::ProfileSource {
            repository_id: "api".into(),
            source_ref: source_ref.clone(),
            code_root: primary.clone(),
            planning_store: store.clone(),
            repositories: repositories.clone(),
        },
        profile: profile.clone(),
        workspace_id: workspace_id.clone(),
        task_id: task_id.clone(),
        cancellation_id: task_id.clone(),
        task_revision: task_revision.clone(),
        comparison: None,
    };
    let request = InvestigationRequestV1 {
        idempotency_key: uuid::Uuid::new_v4().hyphenated().to_string(),
        repository_id: "api".into(),
        source_ref: source_ref.clone(),
        source_sha: source_sha.clone(),
        workspace_id,
        task_id: task_id.clone(),
        expected_task_revision: task_revision.clone(),
        question: "Where is the Python answer function defined?".into(),
        allowed_evidence_classes: vec![EvidenceKind::Source],
        scope: InvestigationScopeV1 {
            paths: vec!["app.py".into()],
        },
        budget: InvestigationBudgetV1 {
            max_files: 2,
            max_bytes: 2048,
            max_matches: 4,
            max_elapsed_ms: 2000,
        },
        cancellation_id: task_id.clone(),
        query: "answer".into(),
        comparison: None,
    };
    let receipt = InvestigationBroker::default()
        .investigate(&context, &request, &AtomicBool::new(false))
        .unwrap();
    let authorization = InvestigationAuthorizationV1 {
        code_root: primary.clone(),
        planning_root: store.root.clone(),
        receipt_root: root.join("receipts"),
        project_id: project_id.hyphenated().to_string(),
        repository_id: "api".into(),
        source_ref: source_ref.clone(),
        source_sha: source_sha.clone(),
        workspace_id: context.workspace_id.clone(),
        task_id,
        cancellation_id: context.cancellation_id.clone(),
        task_revision,
        profile,
        repositories: repositories.clone(),
        approved_comparison_revisions: Vec::new(),
    };
    std::fs::create_dir_all(&authorization.receipt_root).unwrap();
    let path = authorization
        .receipt_root
        .join(format!("{}.json", request.idempotency_key));
    let envelope = InvestigationEvidenceEnvelopeV1 { request, receipt };
    std::fs::write(path, serde_json::to_vec(&envelope).unwrap()).unwrap();

    let rows = crate::app::manager::verify_manager_investigation_receipts_for_test(&authorization)
        .unwrap();
    let (evidence_id, content_sha256, content) = rows
        .into_iter()
        .find(|(id, _, _)| id.starts_with("investigation:"))
        .expect("broker file evidence is handed off to the baseline");
    let evidence = crate::domain::chatlog::VerifiedRepositoryEvidence {
        evidence_id,
        repository_id: "api".into(),
        source_ref,
        source_sha,
        content_sha256,
        excerpt_sha256: format!("{:x}", sha2::Sha256::digest(content.as_bytes())),
        content,
    };
    crate::core::specification_chat::validate_reviewed_repository_evidence(
        &primary,
        &store,
        &repositories,
        &[evidence],
    )
    .unwrap();
}

#[test]
fn comparison_authority_contains_only_bounded_validated_same_repository_git_intents() {
    let mut metadata_items = Vec::new();
    for index in 0..10 {
        let identity = crate::domain::ArtifactIdentity {
            uid: uuid::Uuid::new_v4().hyphenated().to_string(),
            display_id: format!("T{index}"),
            title: format!("Synthetic task {index}"),
            parent_uid: Some(uuid::Uuid::new_v4().hyphenated().to_string()),
        };
        let metadata =
            crate::artifacts::task_docs::TaskMetadata::new(&identity, "root", Vec::new()).unwrap();
        let intent = crate::core::git_intent::GitIntentV1::approve(
            "root".into(),
            format!("approved-{index}"),
            format!("{index:040x}"),
            "main".into(),
            crate::core::git_intent::PublicationModeV1::HoldForReview,
            None,
        )
        .unwrap();
        metadata_items.push(metadata.with_git_intent(Some(&intent)).unwrap());
    }
    let identity = crate::domain::ArtifactIdentity {
        uid: uuid::Uuid::new_v4().hyphenated().to_string(),
        display_id: "T-other".into(),
        title: "Other repository".into(),
        parent_uid: Some(uuid::Uuid::new_v4().hyphenated().to_string()),
    };
    let other_metadata =
        crate::artifacts::task_docs::TaskMetadata::new(&identity, "root", Vec::new()).unwrap();
    let other_intent = crate::core::git_intent::GitIntentV1::approve(
        "api".into(),
        "api-approved".into(),
        "a".repeat(40),
        "main".into(),
        crate::core::git_intent::PublicationModeV1::HoldForReview,
        None,
    )
    .unwrap();
    metadata_items.push(other_metadata.with_git_intent(Some(&other_intent)).unwrap());

    let pins = approved_comparison_pins(&metadata_items, "root");
    assert_eq!(pins.len(), 8);
    assert!(
        pins.iter()
            .all(|pin| pin.intent.repository_id == "root" && pin.intent.validate().is_ok())
    );
    assert!(
        pins.iter()
            .all(|pin| pin.intent.source_ref.starts_with("approved-"))
    );
}
