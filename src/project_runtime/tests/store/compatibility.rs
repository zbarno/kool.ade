use super::*;

// Serialized by the pre-target-selection schema from #179. In particular,
// schema v1 did not yet include `selected_target_id` on the manifest.
const HISTORIC_MANIFEST_V1: &str = r#"
{
  "schema_version": 1,
  "workspace_id": "workspace-001",
  "repository_id": "repo-main",
  "source_sha": "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
  "detected_languages": ["Python"],
  "frameworks": ["FastAPI"],
  "candidate_targets": [{
    "target_id": "web-target",
    "owning_task_id": "task-001",
    "repository_id": "repo-main",
    "app_surface": "web",
    "project_relative_root": ".",
    "build_recipe_ref": "web-build",
    "test_recipe_refs": [],
    "start_recipe_ref": null,
    "readiness_probe_ref": null,
    "artifact_spec_ref": null,
    "fixture_ref": null,
    "required_dependencies": [],
    "supported_driver_ids": ["browser"],
    "port_needs": [],
    "environment_policy_ref": "qa-approved-port-only",
    "capability_state": "ready_but_untested"
  }],
  "target_capabilities": ["ready_but_untested"],
  "recipes": [{
    "schema_version": 1,
    "recipe_id": "web-build",
    "source": "user_approved",
    "working_directory_relative": ".",
    "executable_id": "python",
    "argv": ["-m", "compileall", "."],
    "allowed_env_keys": [],
    "allowed_output_paths": [],
    "timeout_seconds": 60,
    "expected_exit_codes": [0],
    "expected_artifacts": [],
    "cache_scope": null,
    "prerequisite_tool_ids": ["python"],
    "network_requirement": "denied",
    "evidence_refs": ["user-approval:approval-001"]
  }],
  "readiness_probes": {},
  "artifact_specs": {},
  "confidence": 80,
  "evidence_refs": ["pyproject.toml"],
  "user_overrides": [],
  "revision": 4
}
"#;

#[test]
fn pre_target_selection_manifest_target_and_recipe_remain_readable() {
    let fixture = Fixture::new();
    fixture
        .store
        .atomic_write(
            "state/project-runtime.json",
            HISTORIC_MANIFEST_V1.as_bytes(),
        )
        .unwrap();

    let runtime = ProjectRuntimeStore::new(&fixture.store);
    let manifest = runtime.load_manifest().unwrap().unwrap();
    assert_eq!(manifest.schema_version, 1);
    assert_eq!(manifest.revision, 4);
    assert_eq!(manifest.selected_target_id, None);
    assert!(manifest.is_active_target("task-001", "web-target"));

    let mut ambiguous = manifest.clone();
    let mut second_target = ambiguous.candidate_targets[0].clone();
    second_target.target_id = "api-target".into();
    ambiguous.candidate_targets.push(second_target);
    ambiguous
        .target_capabilities
        .push(crate::project_runtime::TargetCapabilityState::ReadyButUntested);
    assert!(ambiguous.validate().is_ok());
    assert!(!ambiguous.is_active_target("task-001", "web-target"));
    assert!(!ambiguous.is_active_target("task-001", "api-target"));

    assert_eq!(manifest.candidate_targets[0].project_relative_root, ".");
    assert_eq!(manifest.recipes[0].executable_id, "python");
    assert_eq!(
        manifest.recipes[0].argv,
        vec!["-m".to_owned(), "compileall".to_owned(), ".".to_owned()]
    );
    assert!(manifest.recipes[0].is_explicitly_user_approved());

    // Saving an old single-target record preserves its conservative implicit
    // selection semantics while advancing the current revision fence.
    runtime.save_manifest(&manifest, manifest.revision).unwrap();
    let reloaded = runtime.load_manifest().unwrap().unwrap();
    assert_eq!(reloaded.revision, 5);
    assert_eq!(reloaded.selected_target_id, None);
    assert!(reloaded.is_active_target("task-001", "web-target"));
    assert_eq!(reloaded.candidate_targets, manifest.candidate_targets);
    assert_eq!(reloaded.recipes, manifest.recipes);
}

#[test]
fn pre_dependency_receipt_remains_readable_without_inventing_prepared_dependencies() {
    let fixture = Fixture::new();
    let receipt_id = uuid::Uuid::new_v4().hyphenated().to_string();
    // #179 receipts predate `prepared_dependency_ids`; absence is read as an
    // empty list and does not retroactively claim that any dependency was staged.
    let historic_receipt = format!(
        r#"{{
          "schema_version": 1,
          "receipt_id": "{receipt_id}",
          "operation": "build",
          "task_id": "task-001",
          "target_id": "web-target",
          "source_sha": "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb",
          "recipe_id": "web-build",
          "state": "passed",
          "exit_code": 0,
          "artifact_hashes": {{}},
          "log_reference": "log-001",
          "observed_at_unix_ms": 9
        }}"#
    );
    fixture
        .store
        .atomic_create(
            format!("state/runtime-receipts/{receipt_id}.json"),
            historic_receipt.as_bytes(),
        )
        .unwrap();

    let runtime = ProjectRuntimeStore::new(&fixture.store);
    let receipt = runtime.load_receipt(&receipt_id).unwrap().unwrap();
    assert_eq!(receipt.receipt_id, receipt_id);
    assert_eq!(receipt.prepared_dependency_ids, Vec::<String>::new());
    assert_eq!(
        runtime.receipts_for_task("task-001").unwrap(),
        vec![receipt]
    );
}
