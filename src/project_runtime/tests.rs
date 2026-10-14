use super::*;

#[path = "tests/discovery.rs"]
mod discovery;
#[path = "tests/store.rs"]
mod store;

#[test]
fn project_detection_inspects_metadata_without_running_scripts() {
    let root =
        std::env::temp_dir().join(format!("koolade-runtime-contract-{}", uuid::Uuid::new_v4()));
    std::fs::create_dir_all(&root).unwrap();
    std::fs::write(
        root.join("package.json"),
        r#"{"scripts":{"test":"touch executed"}}"#,
    )
    .unwrap();
    let evidence = detect_candidate_targets(&root).unwrap();
    assert_eq!(evidence.len(), 1);
    assert_eq!(evidence[0].language, "JavaScript");
    assert!(!root.join("executed").exists());
    std::fs::remove_dir_all(root).unwrap();
}

#[test]
fn detection_does_not_follow_symlink_metadata() {
    let root = std::env::temp_dir().join(format!("koolade-runtime-link-{}", uuid::Uuid::new_v4()));
    std::fs::create_dir_all(&root).unwrap();
    let target = root.join("external");
    std::fs::create_dir(&target).unwrap();
    #[cfg(unix)]
    std::os::unix::fs::symlink(target.join("Cargo.toml"), root.join("Cargo.toml")).unwrap();
    assert!(detect_candidate_targets(&root).unwrap().is_empty());
    std::fs::remove_dir_all(root).unwrap();
}

#[test]
fn recipe_rejects_shell_evaluation_and_protected_environment() {
    let recipe = ExecutionRecipeV1 {
        schema_version: 1,
        recipe_id: "test-build".into(),
        source: RecipeSource::UserApproved,
        working_directory_relative: ".".into(),
        executable_id: "node".into(),
        argv: vec!["-c".into(), "touch /tmp/host".into()],
        allowed_env_keys: vec!["API_TOKEN".into()],
        allowed_output_paths: vec!["dist".into()],
        timeout_seconds: 60,
        expected_exit_codes: vec![0],
        expected_artifacts: vec!["dist/index.html".into()],
        cache_scope: None,
        prerequisite_tool_ids: vec!["node".into()],
        network_requirement: NetworkRequirement::Denied,
        evidence_refs: vec!["package.json".into()],
    };
    assert!(recipe.validate().is_err());
}
