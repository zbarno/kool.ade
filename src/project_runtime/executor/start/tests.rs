use super::*;
use crate::project_runtime::{RecipeSource, RuntimeOperation, RuntimeReceiptState};

fn recipe() -> crate::project_runtime::ExecutionRecipeV1 {
    crate::project_runtime::ExecutionRecipeV1 {
        schema_version: 1,
        recipe_id: "start-web".into(),
        source: RecipeSource::UserApproved,
        working_directory_relative: "packages/web".into(),
        executable_id: "npm".into(),
        argv: vec![
            "run".into(),
            "start".into(),
            "$(touch /tmp/host)".into(),
            "it's quoted".into(),
        ],
        allowed_env_keys: Vec::new(),
        allowed_output_paths: Vec::new(),
        timeout_seconds: 90,
        expected_exit_codes: vec![0],
        expected_artifacts: Vec::new(),
        cache_scope: None,
        prerequisite_tool_ids: Vec::new(),
        network_requirement: crate::project_runtime::NetworkRequirement::Denied,
        evidence_refs: vec!["user-approval:test-fixture".into()],
    }
}

fn target() -> crate::project_runtime::AppTargetV1 {
    crate::project_runtime::AppTargetV1 {
        target_id: "web".into(),
        owning_task_id: "task".into(),
        repository_id: "repo".into(),
        app_surface: crate::project_runtime::AppSurface::Web,
        project_relative_root: "apps".into(),
        build_recipe_ref: None,
        test_recipe_refs: Vec::new(),
        start_recipe_ref: Some("start-web".into()),
        readiness_probe_ref: Some("web-ready".into()),
        artifact_spec_ref: None,
        fixture_ref: None,
        required_dependencies: Vec::new(),
        supported_driver_ids: Vec::new(),
        port_needs: vec![8080],
        environment_policy_ref: "env-policy".into(),
        capability_state: TargetCapabilityState::Ready,
    }
}

fn manifest() -> ProjectCapabilityManifestV1 {
    let mut readiness_probes = std::collections::BTreeMap::new();
    readiness_probes.insert(
        "web-ready".into(),
        crate::project_runtime::ReadinessProbeV1::ProcessAlive {
            executable_id: "npm".into(),
        },
    );
    ProjectCapabilityManifestV1 {
        schema_version: 1,
        workspace_id: "project".into(),
        repository_id: "repo".into(),
        source_sha: "a".repeat(40),
        detected_languages: Vec::new(),
        frameworks: Vec::new(),
        candidate_targets: vec![target()],
        selected_target_id: None,
        target_capabilities: vec![TargetCapabilityState::Ready],
        recipes: vec![recipe()],
        readiness_probes,
        artifact_specs: Default::default(),
        confidence: 100,
        evidence_refs: Vec::new(),
        user_overrides: Vec::new(),
        revision: 1,
    }
}

fn build_receipt(path: &str) -> RuntimeReceiptV1 {
    RuntimeReceiptV1 {
        schema_version: 1,
        receipt_id: "build-1".into(),
        operation: RuntimeOperation::Build,
        task_id: "task".into(),
        target_id: "web".into(),
        source_sha: "a".repeat(40),
        recipe_id: Some("build-web".into()),
        state: RuntimeReceiptState::Passed,
        exit_code: Some(0),
        artifact_hashes: [(path.into(), "b".repeat(64))].into(),
        prepared_dependency_ids: Vec::new(),
        log_reference: Some("execution-1".into()),
        observed_at_unix_ms: 1,
    }
}

#[test]
fn start_requires_a_build_receipt_matching_target_source_and_artifact_spec() {
    let mut manifest = manifest();
    let target = &mut manifest.candidate_targets[0];
    target.build_recipe_ref = Some("build-web".into());
    target.artifact_spec_ref = Some("web-dist".into());
    manifest
        .artifact_specs
        .insert("web-dist".into(), vec!["dist/index.html".into()]);
    let receipt = build_receipt("apps/dist/index.html");

    let selected = build_artifacts(
        &manifest,
        &manifest.candidate_targets[0],
        std::slice::from_ref(&receipt),
        &"a".repeat(40),
    )
    .unwrap()
    .unwrap();
    assert_eq!(selected.paths, ["dist/index.html"]);
    assert_eq!(selected.hashes["apps/dist/index.html"], "b".repeat(64));
    assert!(
        build_artifacts(
            &manifest,
            &manifest.candidate_targets[0],
            &[],
            &"a".repeat(40),
        )
        .unwrap()
        .is_none()
    );
    assert!(
        build_artifacts(
            &manifest,
            &manifest.candidate_targets[0],
            std::slice::from_ref(&receipt),
            &"c".repeat(40),
        )
        .unwrap()
        .is_none()
    );
}

#[test]
fn start_rejects_build_receipts_with_missing_or_extra_artifact_hashes() {
    let mut manifest = manifest();
    let target = &mut manifest.candidate_targets[0];
    target.build_recipe_ref = Some("build-web".into());
    target.artifact_spec_ref = Some("web-dist".into());
    manifest
        .artifact_specs
        .insert("web-dist".into(), vec!["dist/index.html".into()]);

    let mut receipt = build_receipt("unexpected.bin");
    assert!(
        build_artifacts(
            &manifest,
            &manifest.candidate_targets[0],
            &[receipt.clone()],
            &"a".repeat(40),
        )
        .is_err()
    );
    receipt
        .artifact_hashes
        .insert("apps/dist/index.html".into(), "b".repeat(64));
    assert!(
        build_artifacts(
            &manifest,
            &manifest.candidate_targets[0],
            &[receipt],
            &"a".repeat(40),
        )
        .is_err()
    );
}

#[test]
fn start_uses_build_recipe_outputs_when_the_target_has_no_artifact_spec() {
    let mut manifest = manifest();
    let target = &mut manifest.candidate_targets[0];
    target.build_recipe_ref = Some("build-web".into());
    manifest
        .recipes
        .push(crate::project_runtime::ExecutionRecipeV1 {
            recipe_id: "build-web".into(),
            expected_artifacts: vec!["dist/index.html".into()],
            allowed_output_paths: vec!["dist".into()],
            ..recipe()
        });
    let receipt = build_receipt("apps/dist/index.html");

    let selected = build_artifacts(
        &manifest,
        &manifest.candidate_targets[0],
        &[receipt],
        &"a".repeat(40),
    )
    .unwrap()
    .unwrap();
    assert_eq!(selected.paths, ["dist/index.html"]);
}

#[test]
fn start_script_scopes_working_directory_quotes_argv_and_leaves_process_running() {
    let manifest = manifest();
    let script = start_script(
        &manifest,
        &manifest.candidate_targets[0],
        &manifest.recipes[0],
        manifest.readiness_probes.get("web-ready").unwrap(),
    )
    .unwrap();

    assert!(script.contains("cd '.' || exit 70"));
    assert!(script.contains(
        "cd 'packages/web' && exec 'npm' 'run' 'start' '$(touch /tmp/host)' 'it'\\''s quoted'"
    ));
    assert!(!script.contains("trap cleanup"));
    assert!(!script.contains("kill -TERM \"$pid\""));
    assert!(script.contains("readiness probe passed"));
    assert!(script.contains("[ \"$i\" -lt 40 ]"));
    assert!(script.contains("'$(touch /tmp/host)'"));
}

#[test]
fn http_readiness_uses_only_a_declared_application_owned_probe_tool() {
    let mut manifest = manifest();
    manifest.recipes[0].prerequisite_tool_ids = vec!["node".into()];
    let probe = crate::project_runtime::ReadinessProbeV1::HttpOnApprovedPort {
        port: 8080,
        path: "/health".into(),
        expected_status: 204,
    };
    let command = readiness_command(&manifest, &manifest.candidate_targets[0], &probe).unwrap();
    assert!(command.contains("'node' '-e'"));
    assert!(command.contains("127.0.0.1"));
    assert!(command.contains("path:\"/health\""));
    assert!(command.contains("204"));
    let local_health = crate::project_runtime::ReadinessProbeV1::LocalServiceHealth {
        port: 8081,
        path: "/health".into(),
    };
    let mut without_probe = manifest;
    without_probe.recipes[0].prerequisite_tool_ids.clear();
    let error = readiness_command(
        &without_probe,
        &without_probe.candidate_targets[0],
        &local_health,
    )
    .unwrap_err();
    assert!(error.to_string().contains("node, python, python3, or go"));
}

#[test]
fn http_readiness_uses_the_locked_python_tool_alias() {
    let mut manifest = manifest();
    manifest.recipes[0].executable_id = "python".into();
    manifest.recipes[0].prerequisite_tool_ids = vec!["python".into()];
    let probe = crate::project_runtime::ReadinessProbeV1::HttpOnApprovedPort {
        port: 4173,
        path: "/".into(),
        expected_status: 200,
    };

    let command = readiness_command(&manifest, &manifest.candidate_targets[0], &probe).unwrap();

    assert!(command.contains("'python' '-c'"));
    assert!(command.contains("HTTPConnection"));
    assert!(command.contains("4173"));
    assert!(command.contains("status == 200"));
}

#[test]
fn python_http_readiness_uses_only_the_stdlib_client_and_localhost() {
    let mut manifest = manifest();
    manifest.recipes[0].prerequisite_tool_ids = vec!["python3".into()];
    let probe = crate::project_runtime::ReadinessProbeV1::LocalServiceHealth {
        port: 8081,
        path: "/health?ready=1".into(),
    };
    let command = readiness_command(&manifest, &manifest.candidate_targets[0], &probe).unwrap();
    assert!(command.contains("'python3' '-c'"));
    assert!(command.contains("HTTPConnection"));
    assert!(command.contains("127.0.0.1"));
    assert!(command.contains("8081"));
    assert!(command.contains("status < 400"));
    assert!(!command.contains("curl"));
}

#[path = "tests/build_receipts.rs"]
mod build_receipts;

#[path = "tests/readiness_probe.rs"]
mod readiness_probe;

#[path = "tests/executable_path.rs"]
mod executable_path;
