use super::*;

#[test]
fn persisted_prompt_manifest_fails_closed_on_unknown_version_or_schema_drift() {
    let input = base(
        PromptRole::Implementer,
        RolePayloadV1::Worker {
            assigned_task: "Implement the approved task".into(),
            assigned_subtasks: Vec::new(),
            immutable_baseline: "abc123".into(),
            allowed_execution_tools: vec!["koolade_bash".into()],
        },
    );
    let manifest = render(&input).unwrap().manifest;
    assert_eq!(
        validate_prompt_manifest(&manifest).unwrap(),
        PromptRole::Implementer
    );

    let mut unknown = manifest.clone();
    unknown.role_version = "implementer/v99".into();
    assert!(
        validate_prompt_manifest(&unknown)
            .unwrap_err()
            .contains("unknown prompt version")
    );

    let mut wrong_schema = manifest;
    wrong_schema.output_schema_version = "WorkResultV99".into();
    assert!(
        validate_prompt_manifest(&wrong_schema)
            .unwrap_err()
            .contains("does not match")
    );
}

#[test]
fn implementation_output_requires_current_work_report_schema() {
    let input = base(
        PromptRole::Implementer,
        RolePayloadV1::Worker {
            assigned_task: "Implement the approved task".into(),
            assigned_subtasks: Vec::new(),
            immutable_baseline: "abc123".into(),
            allowed_execution_tools: vec!["koolade_bash".into()],
        },
    );
    let manifest = render(&input).unwrap().manifest;
    let stale = r#"{"schemaVersion":1,"status":"complete","blocker_disposition":"none","summary":"done","acceptance_criteria":[],"verification":[],"remaining":[],"human_choices":[]}"#;
    assert!(
        validate_output_with_manifest(&manifest, stale, &Default::default())
            .unwrap_err()
            .contains("unsupported WorkResultV1 schema version")
    );
}
