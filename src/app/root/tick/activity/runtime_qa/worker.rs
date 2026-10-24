use std::{
    fs::{self, OpenOptions},
    path::Path,
    sync::{Arc, atomic::AtomicBool, mpsc},
};

use fs2::FileExt;

use super::request::PreparedRequest;
use crate::{app::session::Project, runtime_qa::QaScenarioV1};
use run::run;

#[cfg(test)]
pub(crate) use run::{RootlessDesktopDispatch, prepare_rootless_desktop_dispatch};

mod reviewer;
mod run;
#[cfg(test)]
#[path = "worker/tests.rs"]
mod tests;

#[derive(Debug, Eq, PartialEq)]
pub(super) enum StartError {
    ActiveWorker,
    ReportPresent,
    Failed(String),
}

#[derive(Debug, Eq, PartialEq)]
pub(super) enum RunLockError {
    ActiveWorker,
    Failed(String),
}

impl From<RunLockError> for StartError {
    fn from(error: RunLockError) -> Self {
        match error {
            RunLockError::ActiveWorker => Self::ActiveWorker,
            RunLockError::Failed(reason) => Self::Failed(reason),
        }
    }
}

fn acquire_worker_lock(lock_path: &Path) -> Result<std::fs::File, RunLockError> {
    fs::create_dir_all(lock_path.parent().expect("run lock has parent")).map_err(|error| {
        RunLockError::Failed(format!("QA run lock directory unavailable: {error}"))
    })?;
    let lock = OpenOptions::new()
        .create(true)
        .truncate(false)
        .read(true)
        .write(true)
        .open(lock_path)
        .map_err(|error| {
            RunLockError::Failed(format!("QA run lock could not be opened: {error}"))
        })?;
    match lock.try_lock_exclusive() {
        Ok(()) => Ok(lock),
        Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
            Err(RunLockError::ActiveWorker)
        }
        Err(error) => Err(RunLockError::Failed(format!(
            "QA run lock could not be acquired: {error}"
        ))),
    }
}

pub(super) fn acquire_run_lock(lock_path: &Path) -> Result<std::fs::File, RunLockError> {
    acquire_worker_lock(lock_path)
}

pub(super) fn acquire_lock_and_claim(
    lock_path: &Path,
    lifecycle: &crate::core::lifecycle::LifecycleStore<'_>,
    task_uid: &str,
    expected_revision: u64,
    request: &crate::runtime_qa::QaRunRequestV1,
) -> Result<(std::fs::File, crate::core::lifecycle::TaskRecord), StartError> {
    let lock = acquire_worker_lock(lock_path).map_err(StartError::from)?;
    let store_root = lock_path
        .parent()
        .and_then(Path::parent)
        .ok_or_else(|| StartError::Failed("QA run lock has no evidence store root".into()))?;
    let store = crate::runtime_qa::RuntimeQaStore::new(store_root);
    let running_restart = lifecycle.load_task(task_uid).is_ok_and(|task| {
        task.revision == expected_revision
            && task.phase == crate::core::lifecycle::TaskPhase::AwaitingRuntimeQa
            && task.qa_execution.as_ref().is_some_and(|execution| {
                execution.status == crate::core::lifecycle::QaExecutionStatusV1::Running
                    && execution.request.as_ref() == Some(request)
            })
    });
    match super::reports::persisted_report_state(&store, request, running_restart) {
        super::reports::PersistedReportState::Missing => {}
        super::reports::PersistedReportState::Recordable => {
            return Err(StartError::ReportPresent);
        }
        super::reports::PersistedReportState::Invalid => {
            return Err(StartError::Failed(
                "the persisted Runtime QA report is absent for a stale Running attempt or is partial, legacy, mismatched, or lacks its required reviewer receipt; create a fresh reviewed request".into(),
            ));
        }
    }
    let namespace = uuid::Uuid::parse_str(task_uid)
        .map_err(|_| StartError::Failed("runtime QA task identity is invalid".into()))?;
    let event_uid = uuid::Uuid::new_v5(
        &namespace,
        format!("runtime-qa-worker-start-v1:{}", request.qa_run_id).as_bytes(),
    )
    .hyphenated()
    .to_string();
    let task = lifecycle
        .claim_runtime_qa_worker(task_uid, expected_revision, &event_uid, request)
        .map_err(|error| {
            StartError::Failed(format!("runtime QA dispatch claim failed: {error}"))
        })?;
    Ok((lock, task))
}

pub(super) fn start(
    project: &Project,
    prepared: PreparedRequest,
    scenarios: Vec<QaScenarioV1>,
    harness: Box<dyn crate::harness::AiHarness>,
    expected_revision: u64,
) -> Result<crate::runtime_qa::QaWorkerHandle, StartError> {
    let request = prepared.request.clone();
    let store_root = crate::runtime_qa::RuntimeQaStore::local_root_for_workspace(
        project.state.planning_store.project_id,
    )
    .map_err(|error| {
        StartError::Failed(format!(
            "local runtime QA evidence storage is unavailable: {error}"
        ))
    })?;
    let lock_path = store_root.join(&request.qa_run_id).join("worker.lock");
    let lifecycle = crate::core::lifecycle::LifecycleStore::new(&prepared.planning_store);
    let (lock, _) = acquire_lock_and_claim(
        &lock_path,
        &lifecycle,
        &request.parent_task_uid,
        expected_revision,
        &request,
    )?;
    let cancel = Arc::new(AtomicBool::new(false));
    let worker_cancel = Arc::clone(&cancel);
    let (cancel_sender, cancel_receiver) = tokio::sync::watch::channel(false);
    let (progress_sender, progress_receiver) = mpsc::channel();
    let (sender, receiver) = mpsc::channel();
    std::thread::Builder::new()
        .name(format!("qa-{}", request.qa_run_id))
        .spawn(move || {
            let _lock = lock;
            let result = run(
                prepared,
                scenarios,
                harness,
                store_root,
                worker_cancel,
                cancel_receiver,
                progress_sender,
            );
            let _ = sender.send(result);
        })
        .map_err(|error| {
            StartError::Failed(format!("Runtime QA worker could not start: {error}"))
        })?;
    Ok(crate::runtime_qa::QaWorkerHandle::new(
        request.qa_run_id,
        request.parent_task_uid,
        cancel,
        cancel_sender,
        progress_receiver,
        receiver,
    ))
}
