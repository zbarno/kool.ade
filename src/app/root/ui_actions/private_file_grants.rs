use super::{KooladeApp, Screen};

impl KooladeApp {
    pub(super) fn record_remote_private_file_grant(
        &mut self,
        ticket: &str,
        worker_id: &str,
        selected_paths: &[String],
    ) {
        let result = match &self.screen {
            Screen::Connected(project) => record(project, ticket, worker_id, selected_paths),
            Screen::Welcome => Err(anyhow::anyhow!(
                "Connect a project before approving private files."
            )),
        };
        match result {
            Ok(grant) => self.toasts.success(format!(
                "Approved {} file(s) for remote runs on worker {}. Files are sent only when the task starts.",
                grant.files.len(),
                grant.worker_id
            )),
            Err(error) => self.toasts.warning(format!(
                "Could not record remote private-file approval: {error}"
            )),
        }
    }

    pub(super) fn revoke_remote_private_file_grant(&mut self, ticket: &str, grant_id: &str) {
        let result = match &self.screen {
            Screen::Connected(project) => revoke(project, ticket, grant_id),
            Screen::Welcome => Err(anyhow::anyhow!(
                "Connect a project before revoking private-file approval."
            )),
        };
        match result {
            Ok(()) => self
                .toasts
                .success(
                    "Revoked private-file approval. Any active remote run will stop at its next controller poll.",
                ),
            Err(error) => self
                .toasts
                .warning(format!("Could not revoke private-file approval: {error}")),
        }
    }
}

fn record(
    project: &crate::app::session::Project,
    ticket: &str,
    worker_id: &str,
    selected_paths: &[String],
) -> anyhow::Result<crate::execution::remote::private_files::RemotePrivateFileGrantV1> {
    use crate::{
        core::implementation::TaskRepositoryKind,
        execution::{
            contract::WorkspaceRef,
            remote::{
                WorkspaceManifestV1,
                private_files::{
                    PrivateFileSpecV1, RemotePrivateFileGrantV1, UserConfirmedPrivateFileGrantV1,
                },
            },
        },
        persistence::{
            execution_records, harness_settings, remote_private_file_grants, remote_workers,
        },
    };

    anyhow::ensure!(!selected_paths.is_empty(), "select at least one file");
    anyhow::ensure!(
        selected_paths.len()
            <= crate::execution::remote::private_files::MAX_PRIVATE_FILE_GRANT_FILES,
        "selected file count exceeds the approval limit"
    );
    let implementation = project
        .implementation_states
        .get(ticket)
        .ok_or_else(|| anyhow::anyhow!("task implementation metadata is unavailable"))?;
    anyhow::ensure!(
        implementation.task_repository_kind == TaskRepositoryKind::Clone
            && implementation.task_repository_ready,
        "remote private-file approval requires a ready app-owned task clone"
    );
    crate::core::implementation::validate_app_owned_task_clone(implementation)?;

    let registration = remote_workers::get(worker_id)?
        .ok_or_else(|| anyhow::anyhow!("selected remote worker is not enrolled"))?;
    let now = harness_settings::current_unix_ms();
    let probe = registration
        .last_probe
        .as_ref()
        .ok_or_else(|| anyhow::anyhow!("selected worker has no successful health probe"))?;
    anyhow::ensure!(
        registration.enabled
            && probe.health == remote_workers::RemoteWorkerHealth::Healthy
            && probe.observed_at_unix_ms <= now
            && now.saturating_sub(probe.observed_at_unix_ms)
                <= crate::execution::remote::scheduler::MAX_HEALTH_AGE_MS,
        "selected worker is disabled, unhealthy, or has stale health"
    );

    let workspace = WorkspaceRef {
        project_id: implementation
            .workspace_uid
            .clone()
            .ok_or_else(|| anyhow::anyhow!("task workspace identity is unavailable"))?,
        task_id: implementation
            .task_uid
            .clone()
            .unwrap_or_else(|| super::super::task_target::key(project, ticket)),
        source_sha: implementation.base_commit.clone(),
    };
    workspace.validate()?;
    let snapshot =
        WorkspaceManifestV1::capture(workspace.clone(), &implementation.task_repository)?;
    let mut selected = selected_paths.to_vec();
    selected.sort();
    selected.dedup();
    anyhow::ensure!(
        selected.len() == selected_paths.len(),
        "selected private-file paths contain duplicates"
    );
    let candidates = crate::execution::remote::private_files::discover_private_files(
        &workspace,
        &implementation.task_repository,
        &snapshot,
    )?;
    let files = selected
        .iter()
        .map(|path| {
            candidates
                .iter()
                .find(|candidate| candidate.path == *path)
                .cloned()
                .ok_or_else(|| {
                    anyhow::anyhow!("selected path is no longer an excluded private file")
                })
        })
        .collect::<Result<Vec<_>, _>>()?;
    let now = harness_settings::current_unix_ms();
    let controller_id = execution_records::owner_instance_id().map_err(anyhow::Error::msg)?;
    let grant = RemotePrivateFileGrantV1 {
        grant_id: uuid::Uuid::new_v4().hyphenated().to_string(),
        controller_id: controller_id.clone(),
        operator_id: String::new(),
        worker_id: worker_id.to_owned(),
        worker_host_key_fingerprint: registration.host_key_fingerprint,
        workspace: workspace.clone(),
        confirmed_at_unix_ms: 0,
        expires_at_unix_ms: now
            .saturating_add(crate::execution::remote::private_files::MAX_PRIVATE_FILE_GRANT_TTL_MS),
        files: files
            .into_iter()
            .map(|file| PrivateFileSpecV1 {
                path: file.path,
                size_bytes: file.size_bytes,
                sha256: file.sha256,
                data_class: file.data_class,
            })
            .collect(),
    };
    let confirmed = UserConfirmedPrivateFileGrantV1::after_confirmation(grant, controller_id, now)?;
    remote_private_file_grants::record_user_confirmation(
        &crate::persistence::state_root(),
        confirmed,
        &snapshot.excluded_paths,
    )
}

fn revoke(
    project: &crate::app::session::Project,
    ticket: &str,
    grant_id: &str,
) -> anyhow::Result<()> {
    use crate::persistence::{harness_settings, remote_private_file_grants};

    let implementation = project
        .implementation_states
        .get(ticket)
        .ok_or_else(|| anyhow::anyhow!("task implementation metadata is unavailable"))?;
    let project_id = implementation
        .workspace_uid
        .clone()
        .ok_or_else(|| anyhow::anyhow!("task workspace identity is unavailable"))?;
    let task_id = implementation
        .task_uid
        .clone()
        .unwrap_or_else(|| super::super::task_target::key(project, ticket));
    let now = harness_settings::current_unix_ms();
    let grant = remote_private_file_grants::active_for_task(
        &crate::persistence::state_root(),
        &project_id,
        &task_id,
        now,
    )?
    .into_iter()
    .find(|grant| grant.grant_id == grant_id)
    .ok_or_else(|| anyhow::anyhow!("private-file approval is no longer active for this task"))?;
    remote_private_file_grants::revoke(
        &crate::persistence::state_root(),
        &project_id,
        &grant.grant_id,
        now,
        remote_private_file_grants::PrivateFileGrantRevocationReasonV1::OperatorRevoked,
    )?;
    Ok(())
}
