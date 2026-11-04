use super::{ClaimError, ClaimExecutionAttemptStatus, ClaimLease, ClaimLeaseHandle, ClaimRequest};
use crate::execution::session::AuthorizationGenerationVerifier;
use std::{fs, path::PathBuf, process::Command};

mod local_fence {
    use super::*;
    use std::path::Path;

    fn temp_state_root(name: &str) -> PathBuf {
        let root = std::env::temp_dir().join(format!(
            "koolade-task-fence-{name}-{}",
            uuid::Uuid::new_v4()
        ));
        fs::create_dir_all(&root).expect("create private state root");
        root
    }

    fn request(root: &Path, project: uuid::Uuid, task: &str) -> ClaimRequest {
        ClaimRequest::new_at(root.to_path_buf(), project, task.into())
    }

    #[test]
    fn same_task_is_exclusive_between_processes_and_released_on_drop() {
        const ROOT_ENV: &str = "KOOLADE_TEST_TASK_FENCE_ROOT";
        const PROJECT_ENV: &str = "KOOLADE_TEST_TASK_FENCE_PROJECT";
        const TASK_ENV: &str = "KOOLADE_TEST_TASK_FENCE_TASK";
        if let (Ok(root), Ok(project), Ok(task)) = (
            std::env::var(ROOT_ENV),
            std::env::var(PROJECT_ENV),
            std::env::var(TASK_ENV),
        ) {
            let request = request(
                &PathBuf::from(root),
                uuid::Uuid::parse_str(&project).expect("project id"),
                &task,
            );
            assert!(matches!(request.acquire(), Err(ClaimError::AlreadyClaimed)));
            return;
        }

        let root = temp_state_root("process");
        let project = uuid::Uuid::new_v4();
        let task = "task-shared-between-processes";
        let first = request(&root, project, task)
            .acquire()
            .expect("first fence");
        let output = Command::new(std::env::current_exe().expect("test executable"))
            .args([
                "--exact",
                "core::task_claim::tests::local_fence::same_task_is_exclusive_between_processes_and_released_on_drop",
            ])
            .env(ROOT_ENV, &root)
            .env(PROJECT_ENV, project.to_string())
            .env(TASK_ENV, task)
            .output()
            .expect("start contender process");
        assert!(
            output.status.success(),
            "child did not observe task contention: {}",
            String::from_utf8_lossy(&output.stderr)
        );
        drop(first);
        let second = request(&root, project, task)
            .acquire()
            .expect("fence is released when its owner exits scope");
        drop(second);
        fs::remove_dir_all(root).expect("remove temporary state");
    }

    #[test]
    fn distinct_project_and_task_keys_do_not_contend() {
        let root = temp_state_root("keys");
        let project = uuid::Uuid::new_v4();
        let same_project = request(&root, project, "task-a")
            .acquire()
            .expect("first task fence");
        let other_task = request(&root, project, "task-b")
            .acquire()
            .expect("different task fence");
        let other_project = request(&root, uuid::Uuid::new_v4(), "task-a")
            .acquire()
            .expect("different project fence");
        drop((same_project, other_task, other_project));
        fs::remove_dir_all(root).expect("remove temporary state");
    }

    #[test]
    fn replacing_the_lock_record_revokes_publication_verification() {
        let root = temp_state_root("replace");
        let lease =
            ClaimLease::acquire(&root, uuid::Uuid::new_v4(), "task-replaced").expect("task fence");
        fs::write(&lease.lock_path, "replacement token").expect("replace token");
        assert!(lease.verify().is_err());
        drop(lease);
        fs::remove_dir_all(root).expect("remove temporary state");
    }

    #[test]
    fn execution_authorization_generation_is_bound_to_the_local_fence() {
        let root = temp_state_root("authorization");
        let handle = ClaimLeaseHandle::new(
            request(&root, uuid::Uuid::new_v4(), "task-auth")
                .acquire()
                .expect("task fence"),
        );
        let initial = handle.generation();
        let attempt_id = uuid::Uuid::new_v4().hyphenated().to_string();
        let generation = handle
            .begin_remote_attempt(&attempt_id, "worker-test")
            .expect("start execution attempt");
        assert_ne!(generation, initial);
        AuthorizationGenerationVerifier::verify(&handle, &generation)
            .expect("verify active attempt");
        handle.mark_remote_attempt_started().expect("mark started");
        assert_eq!(
            handle.execution_attempt_for_test().unwrap().status,
            ClaimExecutionAttemptStatus::Running
        );
        handle
            .confirm_remote_attempt_lost()
            .expect("revoke lost attempt");
        assert!(AuthorizationGenerationVerifier::verify(&handle, &generation).is_err());
        drop(handle);
        fs::remove_dir_all(root).expect("remove temporary state");
    }
}
