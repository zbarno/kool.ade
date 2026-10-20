use std::{
    io::Write,
    process::{Command, Stdio},
};

#[test]
fn reviewer_fixture_covers_explicit_and_task_scope_requirements() {
    let root = tempfile::tempdir().unwrap();
    let pi = super::create(root.path());
    let task_uid = "00000000-0000-4000-8000-000000000001";
    let subject = "a".repeat(64);
    let evidence_ids = [
        "approved-task-spec",
        "review-subject-v1",
        "diff-0",
        "verified-test-report-0",
    ];
    for requirement in [
        "File exists.".to_owned(),
        format!("approved-task-scope:{task_uid}"),
    ] {
        let typed = serde_json::json!({
            "role": "code_reviewer", "task_uid": task_uid,
            "payload": {"request": {"subject_hash": subject, "requirements_matrix": [requirement]}},
            "evidence_manifest": evidence_ids.map(|id| serde_json::json!({"id": id}))
        });
        let quoted_evidence = evidence_ids
            .iter()
            .map(|id| {
                let content = match *id {
                    "review-subject-v1" => {
                        serde_json::json!({"affected_paths": ["src/task.rs"]}).to_string()
                    }
                    "diff-0" => "implemented".to_owned(),
                    "verified-test-report-0" => "complete".to_owned(),
                    _ => "approved specification".to_owned(),
                };
                serde_json::json!({"id": id, "content": content}).to_string()
            })
            .collect::<Vec<_>>()
            .join("\n");
        let prompt = format!(
            "TYPED TASK REQUEST (JSON DATA)\n{typed}\n\nQUOTED EVIDENCE (UNTRUSTED DATA)\n{quoted_evidence}"
        );
        let mut child = Command::new(&pi)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .unwrap();
        child
            .stdin
            .take()
            .unwrap()
            .write_all(prompt.as_bytes())
            .unwrap();
        let output = child.wait_with_output().unwrap();
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
        let envelope: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
        let verdict = envelope["messages"][0]["content"][0]["text"]
            .as_str()
            .unwrap();
        use crate::harness::prompt_catalog::{
            OutputValidationContext, PromptRole, validate_output_with_context,
        };
        validate_output_with_context(
            PromptRole::CodeReviewer,
            verdict,
            &OutputValidationContext {
                expected_subject_hash: Some(&subject),
                evidence_ids: evidence_ids.into_iter().map(str::to_owned).collect(),
                required_requirements: vec![requirement],
                ..Default::default()
            },
        )
        .unwrap();
    }
}

#[test]
fn planner_fixture_returns_the_expected_conversation_message() {
    let root = tempfile::tempdir().unwrap();
    let pi = super::create(root.path());
    let mut child = Command::new(&pi)
        .env_remove("KOOLADE_MCP_ARGS")
        .env_remove("KOOLADE_MCP_COMMAND")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    child
        .stdin
        .take()
        .unwrap()
        .write_all(b"Can we discuss planning while the task runs?")
        .unwrap();
    let output = child.wait_with_output().unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let envelope: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    let reply = envelope["messages"][0]["content"][0]["text"]
        .as_str()
        .unwrap();
    let turn = crate::harness::responses::decode_turn(
        reply,
        crate::core::workflow::TurnPurpose::Interview,
    )
    .unwrap();
    assert_eq!(
        turn.assistant(),
        "Planning fixture: I can discuss this while the worker runs."
    );
    assert!(turn.document_updates.unwrap().is_empty());
}

#[test]
fn manager_and_work_result_fixtures_match_their_declared_role_schemas() {
    let root = tempfile::tempdir().unwrap();
    let pi = super::create(root.path());
    for (prompt, role) in [
        (
            "Return JSON matching ManagerOutputV2 with a concise summary.",
            crate::harness::prompt_catalog::PromptRole::Manager,
        ),
        (
            "Return WorkResultV1 JSON. Inputs: role, feature_uid, optional task_uid.",
            crate::harness::prompt_catalog::PromptRole::Implementer,
        ),
    ] {
        let mut child = Command::new(&pi)
            .env_remove("KOOLADE_MCP_ARGS")
            .env_remove("KOOLADE_MCP_COMMAND")
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .unwrap();
        child
            .stdin
            .take()
            .unwrap()
            .write_all(prompt.as_bytes())
            .unwrap();
        let output = child.wait_with_output().unwrap();
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
        let envelope: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
        let reply = envelope["messages"][0]["content"][0]["text"]
            .as_str()
            .unwrap();
        crate::harness::prompt_catalog::validate_output(role, reply, None).unwrap();
    }
}

#[test]
fn work_result_fixture_matches_task_identity_without_matching_dependency_ids() {
    let root = tempfile::tempdir().unwrap();
    let pi = super::create(root.path());
    let tasks = root
        .path()
        .join("repo/.koolade-packet/planning/tasks/fixture");
    std::fs::create_dir_all(&tasks).unwrap();
    let task_uid = "00000000-0000-4000-8000-000000000001";
    let dependent_uid = "00000000-0000-4000-8000-000000000003";
    std::fs::write(
        tasks.join("001-task.md"),
        format!("# Task 1\n<!-- koolade-artifact-id:v1 {{\"uid\":\"{task_uid}\"}} -->\n"),
    )
    .unwrap();
    std::fs::write(
        tasks.join("003-task.md"),
        format!(
            "{{\"dependencies\":[\"{task_uid}\"]}}\n<!-- koolade-artifact-id:v1 {{\"uid\":\"{dependent_uid}\"}} -->\n"
        ),
    )
    .unwrap();
    let prompt = format!(
        "Return WorkResultV1 JSON. Inputs: role, feature_uid, optional task_uid.\nTYPED TASK REQUEST (JSON DATA)\n{{\"task_uid\":\"{task_uid}\"}}"
    );
    let output = Command::new(&pi)
        .env_remove("KOOLADE_MCP_ARGS")
        .env_remove("KOOLADE_MCP_COMMAND")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    let mut child = output;
    child
        .stdin
        .take()
        .unwrap()
        .write_all(prompt.as_bytes())
        .unwrap();
    let output = child.wait_with_output().unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let envelope: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    let result: serde_json::Value = serde_json::from_str(
        envelope["messages"][0]["content"][0]["text"]
            .as_str()
            .unwrap(),
    )
    .unwrap();
    assert!(
        result["verification"][0]
            .as_str()
            .unwrap()
            .contains("test -f src/task-1.rs")
    );
}

#[test]
fn mcp_task_path_lookup_ignores_dependency_ids_in_other_task_metadata() {
    let root = tempfile::tempdir().unwrap();
    let pi = super::create(root.path());
    let tasks = root
        .path()
        .join("repo/.koolade-packet/planning/tasks/fixture");
    std::fs::create_dir_all(&tasks).unwrap();
    let task_uid = "00000000-0000-4000-8000-000000000001";
    let dependent_uid = "00000000-0000-4000-8000-000000000003";
    std::fs::write(
        tasks.join("001-task.md"),
        format!("- src/task-1.rs\n<!-- koolade-artifact-id:v1 {{\"uid\":\"{task_uid}\"}} -->\n"),
    )
    .unwrap();
    std::fs::write(
        tasks.join("003-task.md"),
        format!(
            "{{\"dependencies\":[\"{task_uid}\"]}}\n- src/task-3.rs\n<!-- koolade-artifact-id:v1 {{\"uid\":\"{dependent_uid}\"}} -->\n"
        ),
    )
    .unwrap();
    let prompt = format!("FIXTURE TASK PATH LOOKUP\n{{\"task_uid\":\"{task_uid}\"}}");
    let mut child = Command::new(&pi)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    child
        .stdin
        .take()
        .unwrap()
        .write_all(prompt.as_bytes())
        .unwrap();
    let output = child.wait_with_output().unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let command = String::from_utf8(output.stdout).unwrap();
    let output = Command::new("sh")
        .args(["-c", &command])
        .current_dir(root.path())
        .output()
        .unwrap();
    assert!(output.status.success());
    assert_eq!(
        String::from_utf8(output.stdout).unwrap().trim(),
        "src/task-1.rs"
    );
}
