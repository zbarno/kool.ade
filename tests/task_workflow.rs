#[path = "task_workflow/support.rs"]
mod support;

use koolade::{
    artifacts::planning_store::{PlanningStore, StoreMode},
    core::{state::PlannerState, turn::TurnOutcome},
};
use support::run;

/// Production boundary: the threaded TurnController must persist planning and
/// generated task artifacts for a real Python-only Git workspace. A validator
/// unit test cannot prove that conversation execution and repository commits
/// avoid project toolchain assumptions. Uses a temporary Git repo and a local
/// fixture harness only; no Docker, network, or model API is required.
#[test]
fn interview_approval_multicall_generation_commit_and_failure_recovery() {
    for failure_mode in [0, 1, 2, 3, 4, 6] {
        let tempdir = tempfile::Builder::new()
            .prefix("koolade_full_workflow_")
            .tempdir()
            .unwrap();
        let root = tempdir.path().join("code");
        let planning_root = tempdir.path().join("planning");
        std::fs::create_dir_all(&root).unwrap();
        std::fs::create_dir_all(&planning_root).unwrap();
        assert!(
            std::process::Command::new("git")
                .args(["init", "-q"])
                .current_dir(&root)
                .status()
                .unwrap()
                .success()
        );
        assert!(
            std::process::Command::new("git")
                .args(["init", "-q", "-b", "main"])
                .current_dir(&planning_root)
                .status()
                .unwrap()
                .success()
        );
        for (key, value) in [
            ("user.name", "Synthetic Test"),
            ("user.email", "test@example.invalid"),
        ] {
            assert!(
                std::process::Command::new("git")
                    .args(["config", key, value])
                    .current_dir(&planning_root)
                    .status()
                    .unwrap()
                    .success()
            );
        }
        std::fs::create_dir_all(planning_root.join("config")).unwrap();
        std::fs::write(
            planning_root.join("README.md"),
            "Separate planning repository\n",
        )
        .unwrap();
        std::fs::write(
            planning_root.join("config/repositories.json"),
            serde_json::to_vec_pretty(&koolade::core::project_repos::ProjectManifest {
                repositories: vec![koolade::core::project_repos::Repository {
                    id: "root".into(),
                    role: "Python code".into(),
                    remote: String::new(),
                    display_name: None,
                }],
            })
            .unwrap(),
        )
        .unwrap();
        assert!(
            std::process::Command::new("git")
                .args(["add", "-A"])
                .current_dir(&planning_root)
                .status()
                .unwrap()
                .success()
        );
        assert!(
            std::process::Command::new("git")
                .args(["commit", "-qm", "seed separate planning repository"])
                .current_dir(&planning_root)
                .status()
                .unwrap()
                .success()
        );
        let store = PlanningStore::new(uuid::Uuid::nil(), &planning_root, StoreMode::ManagedLocal);
        std::fs::create_dir_all(root.join("src")).unwrap();
        std::fs::write(
            root.join("src/server.py"),
            "def health():\n    return 'ok'\n",
        )
        .unwrap();
        for (key, value) in [
            ("user.name", "Synthetic Test"),
            ("user.email", "test@example.invalid"),
        ] {
            assert!(
                std::process::Command::new("git")
                    .args(["config", key, value])
                    .current_dir(&root)
                    .status()
                    .unwrap()
                    .success()
            );
        }
        assert!(
            std::process::Command::new("git")
                .args(["add", "src/server.py"])
                .current_dir(&root)
                .status()
                .unwrap()
                .success()
        );
        assert!(
            std::process::Command::new("git")
                .args(["commit", "-qm", "seed Python-only workspace"])
                .current_dir(&root)
                .status()
                .unwrap()
                .success()
        );
        assert!(!root.join("Cargo.toml").exists());
        assert!(!root.join("package.json").exists());
        let mut state = PlannerState::load_with_store(&root, &store).unwrap();
        state.bootstrap_missing().unwrap();
        let TurnOutcome::Applied {
            state,
            commit_result,
            ..
        } = run(state, false, 0)
        else {
            panic!("interview failed")
        };
        assert!(commit_result.is_ok());
        assert!(state.workflow.ready(state.spec_text.as_deref()));
        assert!(!root.join("planning").exists());
        let outcome = run(*state, true, failure_mode);
        if failure_mode == 6 {
            let TurnOutcome::HarnessFailed { error, .. } = outcome else {
                panic!("invalid stories must fail closed")
            };
            assert!(
                error.detail().contains("after 6 attempts"),
                "unexpected generation failure: {}",
                error.detail()
            );
            assert!(
                !planning_root
                    .join("planning/tasks/saved-searches/001-persist-named-search-filters.md")
                    .exists()
            );
        } else if failure_mode == 1 || failure_mode == 2 {
            assert!(matches!(outcome, TurnOutcome::HarnessFailed { .. }));
            let first = planning_root
                .join("planning/tasks/saved-searches/001-persist-named-search-filters.md");
            let saved = std::fs::read_to_string(&first).unwrap();
            let reloaded = PlannerState::load_with_store(&root, &store).unwrap();
            assert!(
                reloaded.workflow.ready(reloaded.spec_text.as_deref()),
                "failed generation can be retried"
            );
            if failure_mode == 1 {
                let manifest: serde_json::Value = serde_json::from_str(
                    &std::fs::read_to_string(
                        planning_root.join("planning/tasks/saved-searches/.koolade-progress.json"),
                    )
                    .unwrap(),
                )
                .unwrap();
                let envelope: koolade::harness::TurnEnvelope =
                    serde_json::from_str(include_str!("fixtures/task-story-1.json")).unwrap();
                let batch = koolade::core::workflow::TaskBatch {
                    brief: reloaded.workflow.brief.clone().unwrap(),
                    specification: reloaded.spec_text.clone().unwrap(),
                    feature_id: None,
                    contract: None,
                    branch_targets: None,
                    task_routing: Default::default(),
                    stories: envelope.task_stories.unwrap(),
                };
                let edited = format!("{saved}\nUser correction\n");
                std::fs::write(&first, &edited).unwrap();
                assert!(
                    koolade::artifacts::task_docs::save_progress(
                        &store,
                        manifest["run"].as_str().unwrap(),
                        &batch,
                        2
                    )
                    .is_err()
                );
                assert_eq!(std::fs::read_to_string(&first).unwrap(), edited);
                std::fs::write(&first, &saved).unwrap();
            }
            let recovered = run(reloaded, true, 5);
            assert!(
                matches!(recovered, TurnOutcome::Applied { .. }),
                "retry must resume the second story with one model call"
            );
            assert_eq!(std::fs::read_to_string(first).unwrap(), saved);
            assert!(
                !planning_root
                    .join("planning/tasks/saved-searches-02")
                    .exists()
            );
            assert!(
                std::fs::read_to_string(
                    planning_root.join("planning/tasks/saved-searches/README.md")
                )
                .unwrap()
                .contains("Status: Complete")
            );
        } else {
            let TurnOutcome::Applied {
                state,
                commit_result,
                ..
            } = outcome
            else {
                panic!("task generation failed: {outcome:#?}")
            };
            assert!(commit_result.is_ok());
            assert_eq!(state.workflow.task_batches[0].count, 2);
            let batch_ref = &state.workflow.task_batches[0];
            let batch_identity = batch_ref.identity.as_ref().expect("batch UID persisted");
            assert!(batch_identity.display_id.starts_with("BATCH-"));
            let batch_root = state.planning_store.root.join(&batch_ref.directory);
            let batch_index = std::fs::read_to_string(batch_root.join("README.md")).unwrap();
            assert_eq!(
                koolade::domain::ArtifactIdentity::from_markdown(&batch_index)
                    .unwrap()
                    .unwrap(),
                *batch_identity
            );
            let first_story =
                std::fs::read_to_string(batch_root.join("001-persist-named-search-filters.md"))
                    .unwrap();
            let story_identity = koolade::domain::ArtifactIdentity::from_markdown(&first_story)
                .unwrap()
                .unwrap();
            assert_ne!(story_identity.uid, batch_identity.uid);
            assert!(
                story_identity
                    .display_id
                    .starts_with("001-persist-named-search-filters")
            );
            let tracked = std::process::Command::new("git")
                .args(["ls-tree", "-r", "--name-only", "HEAD"])
                .current_dir(&planning_root)
                .output()
                .unwrap();
            let paths = String::from_utf8(tracked.stdout).unwrap();
            assert!(
                paths.contains("planning/tasks/saved-searches/001-persist-named-search-filters.md")
            );
            assert!(
                paths
                    .contains("planning/tasks/saved-searches/002-build-the-saved-search-picker.md")
            );
            assert!(paths.contains("planning/tasks/saved-searches/specification.md"));
        }
    }
}
