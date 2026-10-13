use super::*;
use crate::{
    project_runtime::{
        AppSurface, DiscoveryContext, ExecutionRecipeV1, NetworkRequirement, ReadinessProbeV1,
        RecipeSource, RuntimeOperation, apply_user_recipe_override, discover_manifest,
    },
    project_runtime_capability::{
        ApprovedCapabilityInventory, QaApplicability, assess_manifest_target,
    },
};
use sha2::Digest;
use std::collections::BTreeSet;

type Stack = (
    &'static str,
    &'static str,
    AppSurface,
    &'static str,
    Option<&'static str>,
    QaApplicability,
    Option<QaPlatform>,
);

const STACKS: [Stack; 4] = [
    (
        "rust_desktop",
        "Rust",
        AppSurface::LinuxDesktop,
        "cargo",
        Some("linux-desktop-driver"),
        QaApplicability::Visual,
        Some(QaPlatform::LinuxDesktop),
    ),
    (
        "typescript_web",
        "JavaScript",
        AppSurface::Web,
        "node",
        Some("web-driver"),
        QaApplicability::Visual,
        Some(QaPlatform::Web),
    ),
    (
        "python_web",
        "Python",
        AppSurface::Web,
        "python",
        Some("web-driver"),
        QaApplicability::Visual,
        Some(QaPlatform::Web),
    ),
    (
        "go_service",
        "Go",
        AppSurface::HeadlessService,
        "go",
        None,
        QaApplicability::ServiceEvidence,
        None,
    ),
];

fn context() -> DiscoveryContext {
    DiscoveryContext {
        workspace_id: "workspace-runtime-matrix".into(),
        repository_id: "repo-runtime-matrix".into(),
        owning_task_id: uuid::Uuid::new_v4().hyphenated().to_string(),
        source_sha: "a".repeat(40),
    }
}

fn recipe(id: &str, tool: &str, operation: &str, approval: &str) -> ExecutionRecipeV1 {
    ExecutionRecipeV1 {
        schema_version: 1,
        recipe_id: id.into(),
        source: RecipeSource::UserApproved,
        working_directory_relative: ".".into(),
        executable_id: tool.into(),
        argv: vec![operation.into()],
        allowed_env_keys: vec![],
        allowed_output_paths: vec![format!("artifacts/{operation}")],
        timeout_seconds: 60,
        expected_exit_codes: vec![0],
        expected_artifacts: vec![format!("artifacts/{operation}")],
        cache_scope: None,
        prerequisite_tool_ids: vec![tool.into()],
        network_requirement: NetworkRequirement::Denied,
        evidence_refs: vec![approval.into()],
    }
}

fn bind_recipes(
    manifest: &mut crate::project_runtime::ProjectCapabilityManifestV1,
    target_id: &str,
    prefix: &str,
    tool: &str,
    driver: Option<&str>,
) -> String {
    let readiness_id = format!("{prefix}-ready");
    let target = manifest
        .candidate_targets
        .iter_mut()
        .find(|t| t.target_id == target_id)
        .unwrap();
    target.supported_driver_ids = driver.into_iter().map(str::to_owned).collect();
    target.readiness_probe_ref = Some(readiness_id.clone());
    target.port_needs = vec![4173];
    manifest.readiness_probes.insert(
        readiness_id,
        ReadinessProbeV1::LocalServiceHealth {
            port: 4173,
            path: "/health".into(),
        },
    );
    let approval = format!("user-approval:{prefix}");
    for (operation, label) in [
        (RuntimeOperation::Build, "build"),
        (RuntimeOperation::Test, "test"),
        (RuntimeOperation::Start, "start"),
    ] {
        apply_user_recipe_override(
            manifest,
            target_id,
            operation,
            recipe(&format!("{prefix}-{label}"), tool, label, &approval),
        )
        .unwrap();
    }
    manifest
        .candidate_targets
        .iter()
        .find(|t| t.target_id == target_id)
        .unwrap()
        .start_recipe_ref
        .clone()
        .unwrap()
}

fn assert_hash_bound_recipe(
    context: &DiscoveryContext,
    manifest: &crate::project_runtime::ProjectCapabilityManifestV1,
    platform: QaPlatform,
    recipe_id: &str,
) {
    let recipe = manifest
        .user_overrides
        .iter()
        .find(|r| r.recipe_id == recipe_id)
        .unwrap();
    assert_eq!(recipe.source, RecipeSource::UserApproved);
    assert!(
        recipe
            .evidence_refs
            .iter()
            .any(|r| r.starts_with("user-approval:"))
    );
    let hash = format!(
        "{:x}",
        sha2::Sha256::digest(serde_json::to_vec(recipe).unwrap())
    );
    let mut request = super::request();
    request.parent_task_uid = context.owning_task_id.clone();
    request.code_head_sha = context.source_sha.clone();
    request.relevant_platforms = vec![platform];
    request.startup_recipe_ref = recipe_id.into();
    request.startup_recipe_sha256 = hash.clone();
    let target = crate::runtime_qa::QaValidatedTargetV1 {
        task_uid: context.owning_task_id.clone(),
        build_artifact_sha256: request.build_artifact_sha256.clone(),
        startup_recipe_ref: recipe_id.into(),
        startup_recipe_sha256: hash,
        executable: "/workspace/bin/application".into(),
        working_directory: "/workspace".into(),
        arguments: vec!["--qa".into()],
    };
    assert!(target.validate_for(&request).is_ok());
    let mut stale = target;
    stale.startup_recipe_sha256 = "f".repeat(64);
    assert!(stale.validate_for(&request).is_err());
}

#[test]
fn discovered_stack_matrix_uses_approved_recipes_and_stack_specific_qa() {
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/runtime_qa");
    for (fixture, language, surface, tool, driver, applicability, platform) in STACKS {
        let context = context();
        let mut manifest = discover_manifest(&root.join(fixture), &context).unwrap();
        assert_eq!(manifest.detected_languages, [language], "{fixture}");
        assert_eq!(manifest.candidate_targets.len(), 1, "{fixture}");
        let target_id = manifest.candidate_targets[0].target_id.clone();
        let start_id = bind_recipes(
            &mut manifest,
            &target_id,
            fixture.replace('_', "-").as_str(),
            tool,
            driver,
        );
        let tools = [tool.to_owned()].into_iter().collect::<BTreeSet<_>>();
        let drivers = driver
            .into_iter()
            .map(str::to_owned)
            .collect::<BTreeSet<_>>();
        let inventory = ApprovedCapabilityInventory {
            manifest: &manifest,
            available_tool_ids: &tools,
            available_dependency_ids: &BTreeSet::new(),
            available_driver_ids: &drivers,
        };
        let result =
            assess_manifest_target(&manifest, &target_id, &context.owning_task_id, &inventory)
                .unwrap();
        assert_eq!(
            manifest.candidate_targets[0].app_surface, surface,
            "{fixture}"
        );
        assert!(
            result.report.gaps.is_empty(),
            "{fixture}: {:?}",
            result.report.gaps
        );
        assert_eq!(result.report.qa_applicability, applicability, "{fixture}");
        for selected in &manifest.user_overrides {
            assert_eq!(selected.source, RecipeSource::UserApproved);
            assert_eq!(selected.prerequisite_tool_ids, [tool]);
            assert!(
                selected
                    .evidence_refs
                    .iter()
                    .any(|r| r.starts_with("user-approval:"))
            );
        }
        if let Some(platform) = platform {
            assert_hash_bound_recipe(&context, &manifest, platform, &start_id);
        } else {
            assert_eq!(result.report.selected_driver_id, None);
        }
    }
}

#[test]
fn polyglot_browser_targets_keep_independent_tool_recipes_and_hash_bindings() {
    let root =
        std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/runtime_qa/polyglot");
    let context = context();
    let mut manifest = discover_manifest(&root, &context).unwrap();
    assert_eq!(manifest.detected_languages, ["JavaScript", "Python"]);
    assert_eq!(manifest.candidate_targets.len(), 2);
    let tools = ["node".to_owned(), "python".to_owned()]
        .into_iter()
        .collect::<BTreeSet<_>>();
    let drivers = ["web-driver".to_owned()]
        .into_iter()
        .collect::<BTreeSet<_>>();
    let targets = manifest
        .candidate_targets
        .iter()
        .map(|t| (t.target_id.clone(), t.project_relative_root.clone()))
        .collect::<Vec<_>>();
    for (target_id, path) in targets {
        let (prefix, tool) = match path.as_str() {
            "apps/web" => ("polyglot-web", "node"),
            "services/api" => ("polyglot-api", "python"),
            _ => panic!("unexpected target {path}"),
        };
        let start_id = bind_recipes(&mut manifest, &target_id, prefix, tool, Some("web-driver"));
        let inventory = ApprovedCapabilityInventory {
            manifest: &manifest,
            available_tool_ids: &tools,
            available_dependency_ids: &BTreeSet::new(),
            available_driver_ids: &drivers,
        };
        let result =
            assess_manifest_target(&manifest, &target_id, &context.owning_task_id, &inventory)
                .unwrap();
        assert_eq!(
            result.report.qa_applicability,
            QaApplicability::Visual,
            "{path}"
        );
        assert!(
            result.report.gaps.is_empty(),
            "{path}: {:?}",
            result.report.gaps
        );
        assert!(
            manifest
                .user_overrides
                .iter()
                .filter(|r| r.recipe_id.starts_with(prefix))
                .all(|r| r.prerequisite_tool_ids == [tool])
        );
        assert_hash_bound_recipe(&context, &manifest, QaPlatform::Web, &start_id);
    }
}
