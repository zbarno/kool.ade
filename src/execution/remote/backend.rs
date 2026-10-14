use std::{
    collections::HashMap,
    fs,
    path::{Path, PathBuf},
    sync::Mutex,
};

use crate::execution::{
    backend::ExecutionHandleV1,
    contract::{ExecutionRequestV1, ExecutionTarget, TargetLocation, WorkspaceRef},
    docker::{
        backend::{OwnedTaskWorkspaceResolver, TaskWorkspaceResolver},
        sandbox::{ValidatedWorkspace, WorkspaceSnapshot, staging},
    },
    errors::{ExecutionError, ExecutionErrorKind},
};

use super::{
    WorkspaceManifestV1,
    client::RemoteWorkerClient,
    protocol::EnvelopeKindV1,
    ssh::SshWorkerConfig,
    staging::{StagingReceiver, TransferManifestV1, VerifiedWorkspaceResult, stream_file_chunks},
};

mod apply;
mod backend_impl;

/// Controller-side backend bound to one preconfigured, host-key-verified worker.
/// The worker protocol only carries typed execution values, never filesystem paths.
pub struct RemoteExecutionBackend {
    target: ExecutionTarget,
    client: Mutex<RemoteWorkerClient>,
    source_root: Option<PathBuf>,
    controller_id: String,
    result_root: PathBuf,
    snapshots: Mutex<HashMap<WorkspaceRef, WorkspaceManifestV1>>,
    execution_snapshots: Mutex<HashMap<String, WorkspaceManifestV1>>,
    baselines: Mutex<HashMap<WorkspaceRef, (ValidatedWorkspace, WorkspaceSnapshot)>>,
    verified_results: Mutex<HashMap<String, VerifiedWorkspaceResult>>,
}

impl RemoteExecutionBackend {
    pub fn connect(
        config: &SshWorkerConfig,
        target: ExecutionTarget,
    ) -> Result<Self, ExecutionError> {
        target.validate().map_err(|_| incompatible())?;
        match &target.location {
            TargetLocation::Remote { worker_id } if worker_id == &config.stable_id => {}
            _ => return Err(incompatible()),
        }
        Self::connect_inner(config, target, None)
    }

    pub fn connect_with_workspace(
        config: &SshWorkerConfig,
        target: ExecutionTarget,
        source_root: impl AsRef<Path>,
    ) -> Result<Self, ExecutionError> {
        Self::connect_inner(config, target, Some(source_root.as_ref().to_path_buf()))
    }

    fn connect_inner(
        config: &SshWorkerConfig,
        target: ExecutionTarget,
        source_root: Option<PathBuf>,
    ) -> Result<Self, ExecutionError> {
        target.validate().map_err(|_| incompatible())?;
        match &target.location {
            TargetLocation::Remote { worker_id } if worker_id == &config.stable_id => {}
            _ => return Err(incompatible()),
        }
        let controller_id = crate::persistence::execution_records::owner_instance_id()
            .map_err(|_| disconnected())?;
        let result_root = crate::persistence::state_root().join("remote-results");
        fs::create_dir_all(&result_root).map_err(|_| disconnected())?;
        Ok(Self {
            target,
            client: Mutex::new(RemoteWorkerClient::connect(config)?),
            source_root,
            controller_id,
            result_root,
            snapshots: Mutex::new(HashMap::new()),
            execution_snapshots: Mutex::new(HashMap::new()),
            baselines: Mutex::new(HashMap::new()),
            verified_results: Mutex::new(HashMap::new()),
        })
    }

    fn stage_workspace(&self, workspace: &WorkspaceRef) -> Result<(), ExecutionError> {
        let root = self.source_root.as_deref().ok_or_else(|| {
            ExecutionError::new(
                ExecutionErrorKind::PolicyDenied,
                "Remote execution requires an app-owned task clone path",
                false,
            )
        })?;
        let resolver = OwnedTaskWorkspaceResolver::for_current_user(workspace.clone(), root)?;
        let clone_path = resolver.resolve_owned_clone(workspace)?;
        let validated = ValidatedWorkspace::inspect(&clone_path)?;
        let baseline = staging::capture(&validated, 512 * 1024 * 1024, false)?;
        let manifest = WorkspaceManifestV1::capture(workspace.clone(), &clone_path)?;
        self.transfer_workspace(&manifest, &clone_path)?;
        self.baselines
            .lock()
            .map_err(|_| disconnected())?
            .insert(workspace.clone(), (validated, baseline));
        self.snapshots
            .lock()
            .map_err(|_| disconnected())?
            .insert(workspace.clone(), manifest);
        Ok(())
    }

    fn transfer_workspace(
        &self,
        workspace: &WorkspaceManifestV1,
        root: &Path,
    ) -> Result<(), ExecutionError> {
        let worker_id = match &self.target.location {
            TargetLocation::Remote { worker_id } => worker_id,
            TargetLocation::Local => return Err(incompatible()),
        };
        let manifest = TransferManifestV1::workspace(
            format!("controller:{}", self.controller_id),
            format!("worker:{worker_id}"),
            workspace.clone(),
        )
        .map_err(|_| incompatible())?;
        let mut client = self.client.lock().map_err(|_| disconnected())?;
        client.request(
            EnvelopeKindV1::BeginWorkspaceTransfer,
            CONTROL_EXECUTION_ID,
            serde_json::json!({"manifest": manifest}),
        )?;
        for (path, _) in manifest.file_specs() {
            stream_file_chunks(&manifest, root, path, |chunk| {
                client
                    .request(
                        EnvelopeKindV1::WorkspaceTransferChunk,
                        CONTROL_EXECUTION_ID,
                        serde_json::json!({"chunk": chunk}),
                    )
                    .map_err(anyhow::Error::from)?;
                Ok(())
            })
            .map_err(|_| incompatible())?;
        }
        client.request(
            EnvelopeKindV1::FinalizeWorkspaceTransfer,
            CONTROL_EXECUTION_ID,
            serde_json::json!({"transfer_id": manifest.transfer_id}),
        )?;
        Ok(())
    }

    fn request<T: serde::de::DeserializeOwned>(
        &self,
        kind: EnvelopeKindV1,
        execution_id: &str,
        payload: serde_json::Value,
    ) -> Result<T, ExecutionError> {
        let mut client = self.client.lock().map_err(|_| disconnected())?;
        let response = client.request(kind, execution_id, payload)?;
        serde_json::from_value(response.payload).map_err(|_| incompatible())
    }

    fn local_handle(
        &self,
        handle: &ExecutionHandleV1,
    ) -> Result<ExecutionHandleV1, ExecutionError> {
        if handle.target != self.target {
            return Err(incompatible());
        }
        let mut local = handle.clone();
        local.target = ExecutionTarget::local();
        Ok(local)
    }

    fn require_target(&self, target: &ExecutionTarget) -> Result<(), ExecutionError> {
        if target == &self.target {
            Ok(())
        } else {
            Err(incompatible())
        }
    }
}

const CONTROL_EXECUTION_ID: &str = "00000000-0000-4000-8000-000000000000";

fn incompatible() -> ExecutionError {
    ExecutionError::new(
        ExecutionErrorKind::WorkerIncompatible,
        "Remote worker identity or execution payload is incompatible",
        false,
    )
}

fn disconnected() -> ExecutionError {
    ExecutionError::new(
        ExecutionErrorKind::WorkerDisconnected,
        "Remote worker connection is unavailable",
        true,
    )
}
