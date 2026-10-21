use std::{
    fs::{self, OpenOptions},
    sync::{Arc, atomic::AtomicBool, mpsc},
};

use fs2::FileExt;

use super::request::PreparedRequest;
use crate::{app::session::Project, runtime_qa::QaScenarioV1};
use run::run;

mod reviewer;
mod run;

pub(super) fn start(
    project: &Project,
    prepared: PreparedRequest,
    scenarios: Vec<QaScenarioV1>,
    harness: Box<dyn crate::harness::AiHarness>,
) -> Result<crate::runtime_qa::QaWorkerHandle, String> {
    let request = prepared.request.clone();
    let store_root = crate::runtime_qa::RuntimeQaStore::local_root_for_workspace(
        project.state.planning_store.project_id,
    )
    .map_err(|error| format!("local runtime QA evidence storage is unavailable: {error}"))?;
    let lock_path = store_root.join(&request.qa_run_id).join("worker.lock");
    fs::create_dir_all(lock_path.parent().expect("run lock has parent"))
        .map_err(|error| format!("QA run lock directory unavailable: {error}"))?;
    let lock = OpenOptions::new()
        .create(true)
        .truncate(false)
        .read(true)
        .write(true)
        .open(&lock_path)
        .map_err(|error| format!("QA run lock could not be opened: {error}"))?;
    lock.try_lock_exclusive()
        .map_err(|_| "this immutable QA run already has an active worker".to_owned())?;
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
        .map_err(|error| format!("Runtime QA worker could not start: {error}"))?;
    Ok(crate::runtime_qa::QaWorkerHandle::new(
        request.qa_run_id,
        request.parent_task_uid,
        cancel,
        cancel_sender,
        progress_receiver,
        receiver,
    ))
}
