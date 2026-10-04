use super::*;

#[test]
fn references_match_whole_ids_in_headings_bullets_and_tables() {
    let document = "# Scope\nOverview\n- **FR-1** Use corporate SSO.\n  Require MFA.\n- **FR-10** Unrelated reporting.\n## D-2 Authentication decision\nUse the existing directory.\n## D-20 Reporting decision\nUnrelated exports.\n| A-3 | SSO is available |\n| A-30 | Reporting is enabled |";
    let selected = referenced_sections(document, "Implement FR-1 based on D-2 and A-3");
    assert!(selected.contains("Require MFA"));
    assert!(selected.contains("existing directory"));
    assert!(selected.contains("SSO is available"));
    assert!(!selected.contains("Unrelated"));
    assert!(!selected.contains("Reporting is enabled"));
}

#[test]
fn task_story_prompt_includes_explicit_dependency_and_referenced_specification() {
    let root = std::env::temp_dir().join(format!("koolade_task_prompt_{}", std::process::id()));
    let directory = ".koolade-packet/planning/tasks/authentication";
    std::fs::create_dir_all(root.join(directory)).unwrap();
    let mut state = PlannerState::load(&root).unwrap();
    state.spec_text = Some("# Product\n## FR-1 Sign in\nUse corporate SSO.\n## FR-10 Reporting\nUnrelated reporting details.".into());
    state
        .workflow
        .task_batches
        .push(crate::core::workflow::TaskBatchRef {
            identity: None,
            feature: "Authentication".into(),
            directory: directory.into(),
            count: 3,
        });
    std::fs::write(
        root.join(directory).join("001-login.md"),
        "# Sign in\nImplement FR-1.\n## Dependencies\n[Directory](002-directory.md)",
    )
    .unwrap();
    std::fs::write(
        root.join(directory).join("002-directory.md"),
        "# Directory\nProvision the corporate tenant.",
    )
    .unwrap();
    std::fs::write(
        root.join(directory).join("003-reporting.md"),
        "# Reporting\nUNRELATED TASK DETAILS",
    )
    .unwrap();
    let body = prompt(
        &state,
        &format!("{directory}/001-login.md"),
        "Use the existing provider",
        &[("User".into(), "OUR TASK HISTORY".into())],
    )
    .unwrap();
    assert!(body.contains("Use corporate SSO."));
    assert!(body.contains("Provision the corporate tenant."));
    assert!(body.contains("OUR TASK HISTORY"));
    assert!(!body.contains("UNRELATED TASK DETAILS"));
    assert!(!body.contains("Unrelated reporting details"));
    std::fs::remove_dir_all(root).unwrap();
}
