use std::sync::Arc;

use crate::execution::docker::images::{ImageLock, RuntimeProfile};

#[path = "runner/desktop.rs"]
#[cfg(target_os = "linux")]
mod desktop;
#[cfg(target_os = "linux")]
pub(super) use desktop::run_rust_desktop;

#[path = "runner/service.rs"]
mod service;
pub(super) use service::run;

pub(super) fn runtime_session(
    fixture: &super::setup::Fixture,
    lock: &ImageLock,
    profile: RuntimeProfile,
    session_id: &str,
) -> crate::execution::session::ExecutionSession {
    let resolver = Arc::new(
        crate::execution::docker::backend::OwnedTaskWorkspaceResolver::new(
            &fixture.projects_root,
            fixture.workspace.clone(),
            &fixture.task_clone,
        )
        .unwrap(),
    );
    let backend = crate::execution::docker::backend::DockerExecutionBackend::connect(
        super::rootless_action_endpoint(),
        resolver,
        session_id.to_owned(),
    )
    .unwrap();
    let image = lock.select(profile, "amd64").unwrap();
    crate::execution::session::ExecutionSession::new(
        Arc::new(backend),
        fixture.workspace.clone(),
        crate::execution::contract::ExecutionTarget::local(),
        crate::execution::contract::ExecutionModeV1::Verification,
        image.digest.clone(),
        crate::execution::contract::ResourceBudgetV1::default(),
    )
    .unwrap()
}
