use crate::{
    core::self_improvement::{SelfImprovementRunV1, SelfImprovementStore},
    execution::session::AuthorizationGenerationVerifier,
};
use std::{path::Path, sync::Arc};

pub(crate) fn exercise_authorized_docker_if_enabled(
    task_repository: &Path,
    project_slug: &str,
    task_uid: &str,
    source_sha: &str,
    verifier: Arc<dyn AuthorizationGenerationVerifier>,
    runs: &SelfImprovementStore,
    ready: &SelfImprovementRunV1,
) -> bool {
    if std::env::var("KOOLADE_DOCKER_BACKEND_INTEGRATION").as_deref() != Ok("1") {
        return false;
    }
    #[cfg(not(target_os = "linux"))]
    {
        let _ = (
            task_repository,
            project_slug,
            task_uid,
            source_sha,
            verifier,
            runs,
            ready,
        );
        return false;
    }
    #[cfg(target_os = "linux")]
    {
        let runtime_dir =
            std::env::var("XDG_RUNTIME_DIR").expect("rootless Docker requires XDG_RUNTIME_DIR");
        let socket = format!("unix://{runtime_dir}/docker.sock");
        assert!(std::env::var_os("DOCKER_CONTEXT").is_none());
        assert_eq!(std::env::var("DOCKER_HOST").as_deref(), Ok(socket.as_str()));
        let engine = std::process::Command::new("docker")
            .args(["info", "--format", "{{json .SecurityOptions}}"])
            .output()
            .expect("Docker CLI must be installed for the opt-in E2E");
        assert!(
            engine.status.success()
                && String::from_utf8_lossy(&engine.stdout).contains("name=rootless"),
            "E2E requires the approved rootless Docker engine: {}",
            String::from_utf8_lossy(&engine.stderr)
        );
        let session =
            crate::execution::session::ExecutionSession::connect_local_with_authorization(
                crate::execution::contract::WorkspaceRef {
                    project_id: project_slug.into(),
                    task_id: task_uid.into(),
                    source_sha: source_sha.into(),
                },
                task_repository,
                crate::execution::contract::ExecutionModeV1::Implementation,
                crate::execution::docker::images::RuntimeProfile::Rust,
                crate::execution::contract::ResourceBudgetV1::default(),
                Some(verifier),
            )
            .unwrap_or_else(|error| panic!("rootless Docker task session failed: {error}"));
        let output = session
            .run_tool_command(
                "touch .self-improvement-authorized",
                std::time::Duration::from_secs(30),
            )
            .unwrap_or_else(|error| panic!("authorized Docker command failed: {error}"));
        assert_eq!(output.result.exit_code, Some(0), "{}", output.stderr);
        let cancelled = runs
            .apply_event(
                &ready.run_uid,
                ready.revision,
                &uuid::Uuid::new_v4().to_string(),
                crate::core::self_improvement::SelfImprovementEventV1::Cancel,
            )
            .unwrap();
        assert_eq!(
            cancelled.phase,
            crate::core::self_improvement::SelfImprovementPhaseV1::Cancelled
        );
        assert!(
            session
                .run_tool_command(
                    "touch .self-improvement-denied",
                    std::time::Duration::from_secs(30)
                )
                .is_err()
        );
        assert!(
            task_repository
                .join(".self-improvement-authorized")
                .is_file()
        );
        assert!(!task_repository.join(".self-improvement-denied").exists());
        true
    }
}
