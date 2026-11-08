use super::*;

/// Production boundary: Manager Pi launch policy plus the real internal MCP
/// server. Invariant: a Manager provider registers only koolade_investigate,
/// and the server refuses shell calls. Unit tests cannot prove the selected
/// provider args and server config agree. Uses a synthetic profile and local
/// process only; no Docker, network, SSH, or vendor CLI; setup cost is low.
#[test]
fn manager_pi_receives_only_the_scoped_investigation_tool() {
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
    let sha = profile.key.commit_sha.clone();
    let cancellation_id = task_id.clone();
    let task_revision = planning_store.revision().unwrap();
    let auth = crate::core::investigation_broker::InvestigationAuthorizationV1 {
        code_root: repository,
        planning_root: planning_store.root.clone(),
        receipt_root: receipts,
        project_id: project_id.clone(),
        repository_id: "root".into(),
        source_ref: "refs/heads/main".into(),
        source_sha: sha.clone(),
        workspace_id: format!("{project_id}:{task_id}:{sha}"),
        task_id: task_id.clone(),
        cancellation_id,
        task_revision,
        profile,
        repositories,
    };
    let binding = crate::harness::execution_security::session_bridge::SessionBinding {
        session_id: uuid::Uuid::new_v4().hyphenated().to_string(),
        capability_id: uuid::Uuid::new_v4().hyphenated().to_string(),
        task_id,
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
    let mut boundary =
        test_boundary_for_profile(ToolAccess::InvestigationBroker, ExecutionProfile::Manager);
    boundary.app_binary = Some(app_binary.clone());
    boundary.server_config = Some(config_path.clone());
    let mut argv = vec!["pi".into()];
    let mut env = Vec::new();
    boundary
        .configure(CliProvider::Pi, &mut argv, &mut env)
        .unwrap();
    registration::assert_server_registration(CliProvider::Pi, &boundary, &argv, &env).unwrap();
    let listed = invoke_internal_mcp(
        &app_binary,
        &config_path,
        serde_json::json!({
            "jsonrpc":"2.0","id":1,"method":"tools/list","params":{}
        }),
    );
    assert_eq!(listed["result"]["tools"].as_array().unwrap().len(), 1);
    assert_eq!(listed["result"]["tools"][0]["name"], "koolade_investigate");
    let request = crate::core::investigation_broker::InvestigationRequestV1 {
        idempotency_key: uuid::Uuid::new_v4().hyphenated().to_string(),
        repository_id: auth.repository_id.clone(),
        source_ref: auth.source_ref.clone(),
        source_sha: auth.source_sha.clone(),
        workspace_id: auth.workspace_id.clone(),
        task_id: auth.task_id.clone(),
        expected_task_revision: auth.task_revision.clone(),
        question: "Which package is declared?".into(),
        allowed_evidence_classes: vec![crate::core::repository_knowledge::EvidenceKind::Manifest],
        scope: crate::core::investigation_broker::InvestigationScopeV1 {
            paths: vec!["Cargo.toml".into()],
        },
        budget: crate::core::investigation_broker::InvestigationBudgetV1 {
            max_files: 2,
            max_bytes: 4096,
            max_matches: 4,
            max_elapsed_ms: 2000,
        },
        cancellation_id: auth.cancellation_id.clone(),
        query: "broker_fixture".into(),
    };
    let mut request_arguments = serde_json::to_value(&request).unwrap();
    request_arguments
        .as_object_mut()
        .unwrap()
        .remove("cancellation_id");
    let response = invoke_internal_mcp(
        &app_binary,
        &config_path,
        serde_json::json!({
            "jsonrpc":"2.0","id":2,"method":"tools/call","params":{
                "name":"koolade_investigate","arguments":request_arguments
            }
        }),
    );
    assert_eq!(response["result"]["isError"], false, "{response}");
    let receipt: crate::core::investigation_broker::InvestigationReceiptV1 =
        serde_json::from_str(response["result"]["content"][0]["text"].as_str().unwrap()).unwrap();
    assert_eq!(
        receipt.outcome,
        crate::core::investigation_broker::InvestigationOutcomeV1::Findings
    );
    assert_eq!(receipt.source_refs.len(), 1);
    let verified =
        crate::app::manager::verify_manager_investigation_receipts_for_test(&auth).unwrap();
    assert!(
        verified
            .iter()
            .any(|(id, _, _)| id.starts_with("investigation-receipt:"))
    );
    assert!(verified.iter().any(|(_, hash, _)| hash.len() == 64));
    let shell = invoke_internal_mcp(
        &app_binary,
        &config_path,
        serde_json::json!({
            "jsonrpc":"2.0","id":3,"method":"tools/call","params":{"name":"koolade_bash","arguments":{"command":"cat /etc/passwd"}}
        }),
    );
    assert_eq!(shell["result"]["isError"], true);
    fs::remove_dir_all(root).unwrap();
}

fn invoke_internal_mcp(app: &std::path::Path, config: &std::path::Path, request: Value) -> Value {
    use std::io::Write;
    use std::process::{Command, Stdio};
    let mut child = Command::new(app)
        .arg("--internal-mcp-server")
        .arg(config)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .spawn()
        .unwrap();
    let mut input = child.stdin.take().unwrap();
    writeln!(input, "{}", request).unwrap();
    drop(input);
    let output = child.wait_with_output().unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    serde_json::from_slice(&output.stdout).unwrap()
}
