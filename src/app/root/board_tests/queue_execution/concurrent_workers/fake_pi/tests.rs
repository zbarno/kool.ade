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
