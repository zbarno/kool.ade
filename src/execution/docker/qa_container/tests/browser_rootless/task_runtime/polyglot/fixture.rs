use sha2::Digest as _;
use std::{fs, path::Path, path::PathBuf, process::Command};

use crate::{
    app::session::Project,
    artifacts::planning_store::PlanningStore,
    core::lifecycle::TaskRecord,
    execution::contract::WorkspaceRef,
    project_runtime::{DiscoveryContext, ProjectRuntimeStore, ReadinessProbeV1, RuntimeOperation},
    ui::layout::RuntimeTargetApprovalProof,
};

#[path = "recipes.rs"]
mod recipes;

pub(super) struct Fixture {
    pub root: tempfile::TempDir,
    pub projects_root: PathBuf,
    pub task_clone: PathBuf,
    pub planning: PlanningStore,
    pub workspace: WorkspaceRef,
    pub project: Project,
    pub lifecycle_task: TaskRecord,
    pub dependency_cache: crate::execution::docker::qa_container::tests::browser_rootless::task_runtime::setup::prepared_cache::SeededCache,
    _home: crate::test_support::KooladeHomeGuard,
    _approval_cleanup: crate::persistence::runtime_target_approvals::WorkspaceCleanup,
}

impl Fixture {
    pub fn new() -> Self {
        let root = tempfile::tempdir().expect("temporary polyglot runtime fixture");
        let operator_home = root.path().join("operator-home");
        fs::create_dir_all(&operator_home).unwrap();
        let home = crate::test_support::KooladeHomeGuard::set(&operator_home);
        let project_id = uuid::Uuid::new_v4().hyphenated().to_string();
        let task_id = uuid::Uuid::new_v4().hyphenated().to_string();
        let projects_root = root.path().join("projects");
        let task_clone = projects_root
            .join(&project_id)
            .join("task-repositories/fixture")
            .join(&task_id);
        fs::create_dir_all(&task_clone).unwrap();
        initialize_git(&task_clone);
        fs::write(
            task_clone.join("README.md"),
            "Synthetic polyglot task baseline.\n",
        )
        .unwrap();
        git(&task_clone, &["add", "README.md"]);
        git_commit(&task_clone, "Create polyglot task baseline");
        let base_sha = git_text(&task_clone, &["rev-parse", "HEAD"]);
        copy_tree(
            &Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/runtime_qa/polyglot"),
            &task_clone,
        );
        let npm_fixture = super::super::setup::npm_fixture::write_lockfile_and_archive(
            &task_clone.join("apps/web"),
            &root.path().join("broker-input"),
        )
        .expect("write the polyglot Node lockfile and cache probe archive");
        git(&task_clone, &["add", "--all"]);
        git_commit(&task_clone, "Add executable TypeScript and Python targets");
        let source_sha = git_text(&task_clone, &["rev-parse", "HEAD"]);
        let diff = git_output(
            &task_clone,
            &["diff", "--binary", &format!("{base_sha}..{source_sha}")],
        );
        let complete_diff_hash = format!("{:x}", sha2::Sha256::digest(diff));
        let workspace = WorkspaceRef {
            project_id: project_id.clone(),
            task_id: task_id.clone(),
            source_sha: source_sha.clone(),
        };
        let dependency_cache = super::super::setup::prepared_cache::prepare_broker_cache(
            &task_clone,
            &workspace,
            &npm_fixture,
        )
        .expect("prepare the polyglot lockfile through the task-bound dependency broker");
        let approval_cleanup =
            crate::persistence::runtime_target_approvals::WorkspaceCleanup::new(&project_id);
        let planning = PlanningStore::legacy_embedded(
            uuid::Uuid::parse_str(&project_id).unwrap(),
            &root.path().join("project"),
        );
        let runtime = ProjectRuntimeStore::new(&planning);
        let context = DiscoveryContext {
            workspace_id: project_id,
            repository_id: "synthetic-polyglot".into(),
            owning_task_id: task_id,
            source_sha,
        };
        let mut manifest = runtime
            .discover_and_save(&task_clone, &context, 0)
            .expect("discover actual nested polyglot targets");
        assert_eq!(manifest.detected_languages, ["JavaScript", "Python"]);
        assert_eq!(manifest.candidate_targets.len(), 2);

        let node_target = manifest
            .candidate_targets
            .iter_mut()
            .find(|target| target.project_relative_root == "apps/web")
            .expect("discovered TypeScript target");
        node_target.required_dependencies = vec![dependency_cache.request_id.clone()];
        runtime
            .save_manifest(&manifest, manifest.revision)
            .expect("bind the selected Node cache request to its target");
        manifest.revision += 1;

        for target_root in ["apps/web", "services/api"] {
            let target_id = manifest
                .candidate_targets
                .iter()
                .find(|target| target.project_relative_root == target_root)
                .expect("polyglot target root")
                .target_id
                .clone();
            let target = manifest
                .candidate_targets
                .iter()
                .find(|target| target.target_id == target_id)
                .expect("approved polyglot target");
            assert_eq!(target.app_surface, crate::project_runtime::AppSurface::Web);
            for operation in [
                RuntimeOperation::Build,
                RuntimeOperation::Test,
                RuntimeOperation::Start,
            ] {
                let proof = RuntimeTargetApprovalProof::for_test();
                let recipe = recipes::recipe(target_root, operation, &proof.receipt_uid());
                manifest = if operation == RuntimeOperation::Start {
                    runtime
                        .approve_recipe_with_readiness_and_save(
                            &target_id,
                            operation,
                            recipe,
                            Some(ReadinessProbeV1::HttpOnApprovedPort {
                                port: 4173,
                                path: "/".into(),
                                expected_status: 200,
                            }),
                            proof,
                            manifest.revision,
                        )
                        .expect("approve nested target start and readiness")
                } else {
                    runtime
                        .approve_recipe_and_save(
                            &target_id,
                            operation,
                            recipe,
                            proof,
                            manifest.revision,
                        )
                        .expect("approve nested target recipe")
                };
            }
        }

        let mut project = crate::app::session::test_support::project_from(root.path());
        project.state.planning_store = planning.clone();
        let lifecycle_task = super::super::setup::desktop_lifecycle::reviewed_task(
            &mut project,
            &task_clone,
            &workspace,
            &complete_diff_hash,
            &base_sha,
            &context.repository_id,
            crate::project_runtime::AppSurface::Web,
        );
        super::super::setup::prepared_cache::attach_broker_request(
            &mut project,
            &lifecycle_task,
            dependency_cache
                .request
                .as_ref()
                .expect("Node target has broker request evidence"),
        );

        Self {
            root,
            projects_root,
            task_clone,
            planning,
            workspace,
            project,
            lifecycle_task,
            dependency_cache,
            _home: home,
            _approval_cleanup: approval_cleanup,
        }
    }
}

fn copy_tree(source: &Path, destination: &Path) {
    fs::create_dir_all(destination).unwrap();
    for entry in fs::read_dir(source).expect("read checked-in polyglot fixture") {
        let entry = entry.unwrap();
        let source_path = entry.path();
        let destination_path = destination.join(entry.file_name());
        if source_path.is_dir() {
            copy_tree(&source_path, &destination_path);
        } else {
            fs::copy(source_path, destination_path).expect("copy checked-in polyglot fixture file");
        }
    }
}

fn initialize_git(root: &Path) {
    git(root, &["init", "--quiet", "--initial-branch=main"]);
}

fn git(root: &Path, args: &[&str]) {
    let output = Command::new("git")
        .args(args)
        .current_dir(root)
        .output()
        .expect("git is available for the runtime fixture");
    assert!(
        output.status.success(),
        "git {args:?} failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
}

fn git_commit(root: &Path, message: &str) {
    git(
        root,
        &[
            "-c",
            "user.name=Koolade Test Fixture",
            "-c",
            "user.email=koolade-test-fixture@users.noreply.github.com",
            "commit",
            "--quiet",
            "--message",
            message,
        ],
    );
}

fn git_text(root: &Path, args: &[&str]) -> String {
    String::from_utf8(git_output(root, args))
        .expect("git output is UTF-8")
        .trim()
        .to_owned()
}

fn git_output(root: &Path, args: &[&str]) -> Vec<u8> {
    let output = Command::new("git")
        .args(args)
        .current_dir(root)
        .output()
        .expect("git is available for the runtime fixture");
    assert!(output.status.success(), "git {args:?} succeeds");
    output.stdout
}
