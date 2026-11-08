use super::*;

/// Production boundary: app-issued comparison grants through the real MCP
/// server. Invariant: two approved pinned revisions produce verifiable paired
/// evidence, while an unapproved ref is rejected. A broker unit test cannot
/// prove the authorization grant reaches the provider-facing tool. Uses local
/// synthetic Git and processes only; no Docker, network, SSH, or vendor CLI;
/// setup cost is low.
#[test]
fn manager_comparison_reads_only_app_approved_revision_pins() {
    let app_binary = current_application_binary();
    let root = std::env::temp_dir().join(format!("k-{}", uuid::Uuid::new_v4()));
    fs::create_dir(&root).unwrap();
    let receipts = root.join("receipts");
    fs::create_dir(&receipts).unwrap();
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(&receipts, fs::Permissions::from_mode(0o700)).unwrap();
    }
    let repository = root.join("synthetic-repo");
    fs::create_dir(&repository).unwrap();
    let git = |args: &[&str]| {
        let output = std::process::Command::new("git")
            .arg("-C")
            .arg(&repository)
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
    git(&["init", "-q"]);
    git(&["config", "user.name", "Synthetic Test"]);
    git(&["config", "user.email", "synthetic@example.invalid"]);
    fs::write(
        repository.join("Cargo.toml"),
        "[package]\nname = \"broker_fixture\"\nversion = \"0.1.0\"\n",
    )
    .unwrap();
    git(&["add", "."]);
    git(&["commit", "-qm", "fixture"]);
    git(&["branch", "-M", "main"]);
    let primary_sha = git(&["rev-parse", "HEAD"]);
    git(&["checkout", "-qb", "approved-comparison"]);
    fs::write(
        repository.join("Cargo.toml"),
        "[package]\nname = \"comparison_fixture\"\nversion = \"0.1.0\"\n",
    )
    .unwrap();
    git(&["add", "."]);
    git(&["commit", "-qm", "approved comparison revision"]);
    let comparison_sha = git(&["rev-parse", "HEAD"]);
    git(&["checkout", "-q", "main"]);

    let project_uuid = uuid::Uuid::new_v4();
    let project_id = project_uuid.hyphenated().to_string();
    let task_id = uuid::Uuid::new_v4().hyphenated().to_string();
    let repositories = crate::core::project_repos::ProjectManifest {
        repositories: vec![crate::core::project_repos::Repository {
            id: "root".into(),
            role: "Primary code".into(),
            remote: String::new(),
            display_name: None,
        }],
    };
    let planning_store = crate::artifacts::planning_store::PlanningStore::new(
        project_uuid,
        root.join("planning"),
        crate::artifacts::planning_store::StoreMode::ManagedLocal,
    );
    let profile = crate::core::repository_knowledge::scan_profile(
        crate::core::repository_knowledge::ProfileRequest {
            repository_id: "root",
            source_ref: "refs/heads/main",
            code_root: &repository,
            planning_store: &planning_store,
            repositories: &repositories,
            now_unix_ms: 1,
        },
    )
    .unwrap();
    let intent = crate::core::git_intent::GitIntentV1::approve(
        "root".into(),
        "approved-comparison".into(),
        comparison_sha.clone(),
        "main".into(),
        crate::core::git_intent::PublicationModeV1::HoldForReview,
        None,
    )
    .unwrap();
    let auth = crate::core::investigation_broker::InvestigationAuthorizationV1 {
        code_root: repository,
        planning_root: planning_store.root.clone(),
        receipt_root: receipts,
        project_id: project_id.clone(),
        repository_id: "root".into(),
        source_ref: "refs/heads/main".into(),
        source_sha: primary_sha.clone(),
        workspace_id: format!("{project_id}:{task_id}:{primary_sha}"),
        task_id: task_id.clone(),
        cancellation_id: task_id.clone(),
        task_revision: planning_store.revision().unwrap(),
        profile,
        repositories,
        approved_comparison_revisions: vec![
            crate::core::investigation_broker::InvestigationApprovedRevisionV1 { intent },
        ],
    };
    let binding = crate::harness::execution_security::session_bridge::SessionBinding {
        session_id: uuid::Uuid::new_v4().hyphenated().to_string(),
        capability_id: uuid::Uuid::new_v4().hyphenated().to_string(),
        task_id: task_id.clone(),
        workspace_id: auth.workspace_id.clone(),
        access: "investigation_broker".into(),
        target_id: "local".into(),
        target_arch: None,
    };
    let config = crate::harness::execution_security::mcp_server::ServerConfig::investigation_broker(
        &binding,
        auth.clone(),
    );
    let config_path = root.join("server.json");
    fs::write(&config_path, serde_json::to_vec(&config).unwrap()).unwrap();
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(&config_path, fs::Permissions::from_mode(0o600)).unwrap();
    }

    let mut request = crate::core::investigation_broker::InvestigationRequestV1 {
        idempotency_key: uuid::Uuid::new_v4().hyphenated().to_string(),
        repository_id: "root".into(),
        source_ref: auth.source_ref.clone(),
        source_sha: primary_sha.clone(),
        workspace_id: auth.workspace_id.clone(),
        task_id: auth.task_id.clone(),
        expected_task_revision: auth.task_revision.clone(),
        question: "What changed in the approved comparison revision?".into(),
        allowed_evidence_classes: vec![crate::core::repository_knowledge::EvidenceKind::Manifest],
        scope: crate::core::investigation_broker::InvestigationScopeV1 {
            paths: vec!["Cargo.toml".into()],
        },
        budget: crate::core::investigation_broker::InvestigationBudgetV1 {
            max_files: 2,
            max_bytes: 4096,
            max_matches: 4,
            max_elapsed_ms: 2_000,
        },
        cancellation_id: auth.cancellation_id.clone(),
        query: "package".into(),
        comparison: Some(crate::core::investigation_broker::InvestigationRevisionV1 {
            source_ref: "refs/heads/approved-comparison".into(),
            source_sha: comparison_sha.clone(),
        }),
    };
    let response = invoke(&app_binary, &config_path, &mut request);
    assert_eq!(response["result"]["isError"], false, "{response}");
    let receipt: crate::core::investigation_broker::InvestigationReceiptV1 =
        serde_json::from_str(response["result"]["content"][0]["text"].as_str().unwrap()).unwrap();
    assert_eq!(
        receipt.outcome,
        crate::core::investigation_broker::InvestigationOutcomeV1::Findings
    );
    assert!(
        receipt
            .source_refs
            .iter()
            .any(|reference| reference.source_sha == primary_sha)
    );
    assert!(
        receipt
            .source_refs
            .iter()
            .any(|reference| reference.source_sha == comparison_sha)
    );
    assert!(
        receipt
            .findings
            .iter()
            .any(|finding| finding.contains("comparison_fixture"))
    );
    let paired_reference = receipt.comparison_refs.first().unwrap().clone();
    assert_eq!(paired_reference.left_source_sha, primary_sha);
    assert_eq!(paired_reference.right_source_sha, comparison_sha);

    request.idempotency_key = uuid::Uuid::new_v4().hyphenated().to_string();
    request.comparison.as_mut().unwrap().source_ref = "refs/heads/unapproved".into();
    request.comparison.as_mut().unwrap().source_sha = "b".repeat(40);
    let denied = invoke(&app_binary, &config_path, &mut request);
    assert_eq!(denied["result"]["isError"], true);
    assert!(
        denied["result"]["content"][0]["text"]
            .as_str()
            .unwrap()
            .contains("not app-approved")
    );
    let verified =
        crate::app::manager::verify_manager_investigation_receipts_for_test(&auth).unwrap();
    assert!(
        verified
            .iter()
            .any(|(id, _, _)| id.starts_with("investigation-receipt:"))
    );
    let paired_evidence = verified
        .iter()
        .find(|(id, _, _)| id == &paired_reference.evidence_id)
        .unwrap();
    assert!(
        paired_evidence
            .2
            .contains(&paired_reference.left_content_sha256)
    );
    assert!(
        paired_evidence
            .2
            .contains(&paired_reference.right_content_sha256)
    );
    assert!(paired_evidence.2.contains("comparison_fixture"));
    assert!(!verified.iter().any(|(id, _, _)| id.contains(":left:")));
    assert!(!verified.iter().any(|(id, _, _)| id.contains(":right:")));
    fs::remove_dir_all(root).unwrap();
}

fn invoke(
    app: &std::path::Path,
    config: &std::path::Path,
    request: &mut crate::core::investigation_broker::InvestigationRequestV1,
) -> Value {
    let mut arguments = serde_json::to_value(request).unwrap();
    arguments.as_object_mut().unwrap().remove("cancellation_id");
    super::invoke_internal_mcp(
        app,
        config,
        serde_json::json!({
            "jsonrpc":"2.0","id":1,"method":"tools/call","params":{
                "name":"koolade_investigate","arguments":arguments
            }
        }),
    )
}
