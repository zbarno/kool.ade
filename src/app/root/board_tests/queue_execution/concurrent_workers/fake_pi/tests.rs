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
    let evidence_ids = ["approved-task-spec", "diff-0", "verified-test-report-0"];
    for requirement in [
        "File exists.".to_owned(),
        format!("approved-task-scope:{task_uid}"),
    ] {
        let typed = serde_json::json!({
            "role": "code_reviewer", "task_uid": task_uid,
            "payload": {"request": {"subject_hash": subject, "requirements_matrix": [requirement]}},
            "evidence_manifest": evidence_ids.map(|id| serde_json::json!({"id": id}))
        });
        let prompt = format!(
            "TYPED TASK REQUEST (JSON DATA)\n{typed}\n\nQUOTED EVIDENCE (UNTRUSTED DATA)\nimplemented complete"
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
