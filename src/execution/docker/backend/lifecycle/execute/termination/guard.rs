use std::time::Duration;

use crate::execution::docker::{backend::DockerExecutionBackend, sandbox};

/// An aborted capture must not leave an owned container or its task running.
pub(super) struct StopOnDrop {
    docker: bollard::Docker,
    container_id: String,
    execution_id: String,
    owner_id: String,
    armed: bool,
}

impl StopOnDrop {
    pub(super) fn new(
        backend: &DockerExecutionBackend,
        container_id: &str,
        execution_id: &str,
    ) -> Self {
        Self {
            docker: backend.docker.clone(),
            container_id: container_id.into(),
            execution_id: execution_id.into(),
            owner_id: backend.identity.owner_instance_id.clone(),
            armed: true,
        }
    }

    pub(super) fn disarm(&mut self) {
        self.armed = false;
    }
}

impl Drop for StopOnDrop {
    fn drop(&mut self) {
        if !self.armed {
            return;
        }
        let Ok(runtime) = tokio::runtime::Handle::try_current() else {
            return;
        };
        let docker = self.docker.clone();
        let id = self.container_id.clone();
        let owner = self.owner_id.clone();
        let execution = self.execution_id.clone();
        runtime.spawn(async move {
            let inspected = tokio::time::timeout(
                Duration::from_secs(5),
                docker.inspect_container(
                    &id,
                    None::<bollard::query_parameters::InspectContainerOptions>,
                ),
            )
            .await;
            let Ok(Ok(info)) = inspected else { return };
            let labels = info
                .config
                .and_then(|config| config.labels)
                .unwrap_or_default();
            if labels.get(sandbox::OWNER_LABEL) != Some(&owner)
                || labels.get(sandbox::EXECUTION_LABEL) != Some(&execution)
            {
                return;
            }
            let _ = tokio::time::timeout(
                Duration::from_secs(3),
                docker.kill_container(&id, None::<bollard::query_parameters::KillContainerOptions>),
            )
            .await;
        });
    }
}
