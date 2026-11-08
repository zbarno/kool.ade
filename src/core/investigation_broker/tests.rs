use super::*;
use std::{
    path::{Path, PathBuf},
    process::Command,
    sync::atomic::AtomicBool,
};

fn request() -> InvestigationRequestV1 {
    let workspace_project_id = uuid::Uuid::new_v4().hyphenated().to_string();
    let workspace_task_id = uuid::Uuid::new_v4().hyphenated().to_string();
    InvestigationRequestV1 {
        idempotency_key: uuid::Uuid::new_v4().hyphenated().to_string(),
        repository_id: "root".into(),
        source_ref: "main".into(),
        source_sha: "a".repeat(40),
        workspace_id: format!(
            "{workspace_project_id}:{workspace_task_id}:{}",
            "a".repeat(40)
        ),
        task_id: uuid::Uuid::new_v4().hyphenated().to_string(),
        expected_task_revision: "revision-1".into(),
        question: "Where is parsing handled?".into(),
        allowed_evidence_classes: vec![crate::core::repository_knowledge::EvidenceKind::Source],
        scope: InvestigationScopeV1 {
            paths: vec!["src/lib.rs".into()],
        },
        budget: InvestigationBudgetV1 {
            max_files: 4,
            max_bytes: 4096,
            max_matches: 8,
            max_elapsed_ms: 2_000,
        },
        cancellation_id: uuid::Uuid::new_v4().hyphenated().to_string(),
        query: "parse".into(),
    }
}

/// Production boundary: the broker validates the app-selected profile and reads the
/// exact Git blob before returning a hash-bearing receipt. A validator unit test
/// cannot prove Git pinning or content hashes match the indexed evidence. Uses
/// local Git only; no Docker, network, or SSH; setup cost is low.
#[test]
fn broker_returns_hash_verified_evidence_and_reuses_replayed_requests() {
    let temp = Temp::new();
    let repo = temp.path().join("repo");
    std::fs::create_dir_all(&repo).unwrap();
    git(&repo, &["init", "-q"]);
    git(&repo, &["config", "user.name", "Synthetic Test"]);
    git(
        &repo,
        &["config", "user.email", "synthetic@example.invalid"],
    );
    std::fs::write(
        repo.join("Cargo.toml"),
        "[package]\nname = \"broker_fixture\"\nversion = \"0.1.0\"\n",
    )
    .unwrap();
    git(&repo, &["add", "."]);
    git(&repo, &["commit", "-qm", "fixture"]);
    git(&repo, &["branch", "-M", "main"]);
    let project_id = uuid::Uuid::new_v4();
    let planning_store = crate::artifacts::planning_store::PlanningStore::new(
        project_id,
        temp.path().join("planning"),
        crate::artifacts::planning_store::StoreMode::ManagedLocal,
    );
    let repositories = crate::core::project_repos::ProjectManifest {
        repositories: vec![crate::core::project_repos::Repository {
            id: "root".into(),
            role: "Primary code".into(),
            remote: String::new(),
            display_name: None,
        }],
    };
    let source = crate::core::repository_knowledge::ProfileSource {
        repository_id: "root".into(),
        source_ref: "refs/heads/main".into(),
        code_root: repo.clone(),
        planning_store: planning_store.clone(),
        repositories: repositories.clone(),
    };
    let profile = crate::core::repository_knowledge::scan_profile(
        crate::core::repository_knowledge::ProfileRequest {
            repository_id: "root",
            source_ref: "refs/heads/main",
            code_root: &repo,
            planning_store: &planning_store,
            repositories: &repositories,
            now_unix_ms: 1,
        },
    )
    .unwrap();
    let task_id = uuid::Uuid::new_v4().hyphenated().to_string();
    let cancellation_id = uuid::Uuid::new_v4().hyphenated().to_string();
    let authorization = InvestigationContext {
        source,
        profile: profile.clone(),
        workspace_id: format!(
            "{}:{}:{}",
            project_id.hyphenated(),
            task_id,
            profile.key.commit_sha
        ),
        task_id,
        cancellation_id,
        task_revision: planning_store.revision().unwrap(),
    };
    let mut request = request();
    request.repository_id = "root".into();
    request.source_ref = "refs/heads/main".into();
    request.source_sha = profile.key.commit_sha.clone();
    request.workspace_id = authorization.workspace_id.clone();
    request.task_id = authorization.task_id.clone();
    request.expected_task_revision = authorization.task_revision.clone();
    request.cancellation_id = authorization.cancellation_id.clone();
    request.scope.paths = vec!["Cargo.toml".into()];
    request.allowed_evidence_classes =
        vec![crate::core::repository_knowledge::EvidenceKind::Manifest];
    request.query = "broker_fixture".into();
    let broker = InvestigationBroker::default();
    let cancel = AtomicBool::new(false);
    let first = broker
        .investigate(&authorization, &request, &cancel)
        .unwrap();
    let replay = broker
        .investigate(&authorization, &request, &cancel)
        .unwrap();
    assert_eq!(first.outcome, InvestigationOutcomeV1::Findings);
    assert_eq!(first.source_refs.len(), 1);
    assert_eq!(first.source_refs[0].source_sha, profile.key.commit_sha);
    assert_eq!(first, replay);
    broker
        .verify_receipt(&authorization, &request, &first)
        .unwrap();
    let mut tampered = first.clone();
    tampered.findings[0].push_str(" injected instruction");
    assert!(
        broker
            .verify_receipt(&authorization, &request, &tampered)
            .is_err()
    );
    let mut wrong_repository = request.clone();
    wrong_repository.repository_id = "other".into();
    assert!(
        broker
            .investigate(&authorization, &wrong_repository, &cancel)
            .is_err()
    );
    for _ in 1..8 {
        let mut follow_on = request.clone();
        follow_on.idempotency_key = uuid::Uuid::new_v4().hyphenated().to_string();
        assert!(
            broker
                .investigate(&authorization, &follow_on, &cancel)
                .is_ok()
        );
    }
    let mut over_limit = request.clone();
    over_limit.idempotency_key = uuid::Uuid::new_v4().hyphenated().to_string();
    assert!(
        broker
            .investigate(&authorization, &over_limit, &cancel)
            .is_err()
    );
    let status = Command::new("git")
        .arg("-C")
        .arg(&repo)
        .args(["status", "--porcelain"])
        .output()
        .unwrap();
    assert!(status.status.success());
    assert!(
        status.stdout.is_empty(),
        "investigation mutated the source repository"
    );
    std::fs::write(
        repo.join("Cargo.toml"),
        "[package]\nname = \"changed_fixture\"\nversion = \"0.1.0\"\n",
    )
    .unwrap();
    git(&repo, &["add", "."]);
    git(&repo, &["commit", "-qm", "advance fixture source"]);
    let stale = broker
        .investigate(&authorization, &request, &cancel)
        .unwrap();
    assert_eq!(stale.outcome, InvestigationOutcomeV1::StaleSource);
    assert!(stale.source_refs.is_empty());
}

#[test]
fn repository_snippet_redaction_hides_secrets_urls_and_host_paths() {
    let secret = super::helpers::safe_line("api_token = \"sample-private-value\"");
    assert!(!secret.contains("sample-private-value"));
    assert!(secret.contains("omitted"));
    let endpoint = super::helpers::safe_line("see https://internal.example.invalid/path");
    assert!(!endpoint.contains("internal.example.invalid"));
    let host_path = super::helpers::safe_line("from /home/operator/private/config");
    assert!(!host_path.contains("/home/operator"));
}

struct Temp(PathBuf);
impl Temp {
    fn new() -> Self {
        let path =
            std::env::temp_dir().join(format!("koolade-investigation-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&path).unwrap();
        Self(path)
    }
    fn path(&self) -> &Path {
        &self.0
    }
}
impl Drop for Temp {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

fn git(root: &Path, args: &[&str]) {
    let output = Command::new("git")
        .arg("-C")
        .arg(root)
        .args(args)
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "git {args:?}: {}",
        String::from_utf8_lossy(&output.stderr)
    );
}

#[test]
fn request_accepts_bounded_scoped_query() {
    assert!(request().validate().is_ok());
}

#[test]
fn request_rejects_arbitrary_or_ambiguous_paths() {
    for path in [
        "/etc/passwd",
        "../secret",
        "src//lib.rs",
        "src\\lib.rs",
        "src/./lib.rs",
    ] {
        let mut value = request();
        value.scope.paths = vec![path.into()];
        assert!(value.validate().is_err(), "accepted {path}");
    }
}

#[test]
fn request_rejects_unknown_fields_and_excessive_budgets() {
    let json = serde_json::to_value(request()).unwrap();
    let mut injected = json.clone();
    injected["command"] = serde_json::json!("rm -rf /");
    assert!(serde_json::from_value::<InvestigationRequestV1>(injected).is_err());
    let mut oversized = request();
    oversized.budget.max_bytes = u32::MAX;
    assert!(oversized.validate().is_err());
}
