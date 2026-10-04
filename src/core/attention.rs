//! Read-only, cached explanations of an external implementation blocker.
mod brief;
mod generate;
mod validation;
use validation::validate;

use crate::core::implementation::{self, BlockerDisposition, Report, ReportStatus};
use crate::harness::AiHarness;
pub use brief::{Brief, HumanStep, OptionBrief, Recommendation};
use serde::{Deserialize, Serialize};
use std::{
    fs,
    path::{Path, PathBuf},
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
        mpsc::{self, Receiver},
    },
};

#[derive(Clone, Debug)]
pub enum View {
    Loading,
    Ready(Brief),
    Error(String),
}

#[derive(Deserialize, Serialize)]
struct Cache {
    source_fingerprint: String,
    brief: Brief,
}

pub struct Controller {
    result: Receiver<Result<Brief, String>>,
    cancel: Arc<AtomicBool>,
    started: std::time::Instant,
}

impl Controller {
    pub fn start(
        repo: PathBuf,
        ticket: String,
        report_path: PathBuf,
        harness: Box<dyn AiHarness>,
    ) -> Self {
        let (send, result) = mpsc::channel();
        let cancel = Arc::new(AtomicBool::new(false));
        let worker_cancel = cancel.clone();
        std::thread::spawn(move || {
            let outcome = run(
                &repo,
                &ticket,
                &report_path,
                harness.as_ref(),
                worker_cancel,
            )
            .map_err(|error| format!("{error:#}"));
            let _ = send.send(outcome);
        });
        Self {
            result,
            cancel,
            started: std::time::Instant::now(),
        }
    }

    pub fn start_detail(
        repo: PathBuf,
        ticket: String,
        detail: String,
        harness: Box<dyn AiHarness>,
    ) -> Self {
        let (send, result) = mpsc::channel();
        let cancel = Arc::new(AtomicBool::new(false));
        let worker_cancel = cancel.clone();
        std::thread::spawn(move || {
            let outcome = run_detail(&repo, &ticket, &detail, harness.as_ref(), worker_cancel)
                .map_err(|error| format!("{error:#}"));
            let _ = send.send(outcome);
        });
        Self {
            result,
            cancel,
            started: std::time::Instant::now(),
        }
    }

    pub fn poll(&self) -> Option<Result<Brief, String>> {
        match self.result.try_recv() {
            Ok(result) => Some(result),
            Err(mpsc::TryRecvError::Empty)
                if self.started.elapsed() >= std::time::Duration::from_secs(120) =>
            {
                self.cancel.store(true, Ordering::Relaxed);
                Some(Err("Explanation timed out. The saved blocker and next actions remain available. Retry explanation when ready.".into()))
            }
            Err(mpsc::TryRecvError::Empty) => None,
            Err(mpsc::TryRecvError::Disconnected) => Some(Err(
                "The explanation worker stopped unexpectedly. Retry explanation.".into(),
            )),
        }
    }
}

impl Drop for Controller {
    fn drop(&mut self) {
        self.cancel.store(true, Ordering::Relaxed);
    }
}

/// Only the report belonging to this task's implementation state may drive a brief.
pub fn source_path(repo: &Path, ticket: &str, detail: &str) -> Option<PathBuf> {
    if !detail.starts_with("## Waiting for user action") {
        return None;
    }
    let path = detail
        .lines()
        .find_map(|line| line.strip_prefix("Full report: "))?;
    let path = PathBuf::from(path.trim());
    if !path.file_name()?.to_str()?.ends_with("-report.json") || !path.is_file() {
        return None;
    }
    let expected = implementation::state_dir(repo, ticket)
        .ok()?
        .canonicalize()
        .ok()?;
    (path.parent()?.canonicalize().ok()? == expected).then_some(path)
}

pub fn detail_key(repo: &Path, ticket: &str, detail: &str) -> Option<PathBuf> {
    let state = implementation::state_dir(repo, ticket).ok()?;
    Some(state.join(format!("detail-{}", fingerprint(detail.as_bytes()))))
}

fn fingerprint(bytes: &[u8]) -> String {
    let mut hash = 0xcbf29ce484222325u64;
    for byte in bytes {
        hash = (hash ^ u64::from(*byte)).wrapping_mul(0x100000001b3);
    }
    format!("{:016x}-{}", hash, bytes.len())
}

fn cache_path(report_path: &Path) -> PathBuf {
    let stem = report_path
        .file_stem()
        .unwrap_or_default()
        .to_string_lossy();
    report_path.with_file_name(format!("{stem}-attention.json"))
}

fn run(
    repo: &Path,
    ticket: &str,
    report_path: &Path,
    harness: &dyn AiHarness,
    cancel: Arc<AtomicBool>,
) -> anyhow::Result<Brief> {
    let bytes = fs::read(report_path)?;
    let report = implementation::parse_report(std::str::from_utf8(&bytes)?)?;
    anyhow::ensure!(
        report.status == ReportStatus::Blocked,
        "Report is no longer blocked"
    );
    let (worktree, task, documents) = generate::context(repo, ticket, &report);
    let mut source = bytes;
    source.extend_from_slice(task.as_bytes());
    source.extend_from_slice(documents.as_bytes());
    // Prompt edits must invalidate persisted prose even when the report and
    // repository documents are unchanged; otherwise an older generic brief
    // can keep looking hardcoded after the generator has improved.
    source.extend_from_slice(generate::cache_material(&report, &task, &documents).as_bytes());
    let source_fingerprint = fingerprint(&source);
    let sidecar = cache_path(report_path);
    if let Some(cache) = fs::read(&sidecar)
        .ok()
        .and_then(|bytes| serde_json::from_slice::<Cache>(&bytes).ok())
        && cache.source_fingerprint == source_fingerprint
        && validate(&cache.brief, &report).is_ok()
    {
        return Ok(cache.brief);
    }
    let brief = generate::run(
        &worktree,
        &task,
        &documents,
        &report,
        harness,
        cancel.clone(),
    )?;
    validate(&brief, &report)?;
    anyhow::ensure!(!cancel.load(Ordering::SeqCst), "Explanation cancelled");
    let bytes = serde_json::to_vec_pretty(&Cache {
        source_fingerprint,
        brief: brief.clone(),
    })?;
    crate::artifacts::atomic_write_bytes(&sidecar, &bytes)?;
    Ok(brief)
}

fn run_detail(
    repo: &Path,
    ticket: &str,
    detail: &str,
    harness: &dyn AiHarness,
    cancel: Arc<AtomicBool>,
) -> anyhow::Result<Brief> {
    let actions = detail
        .split_once("### Next action(s)")
        .map(|(_, tail)| tail.to_owned())
        .unwrap_or_else(|| detail.to_owned());
    let report = Report {
        status: ReportStatus::Blocked,
        blocker_disposition: BlockerDisposition::HumanAction,
        summary: detail.to_owned(),
        acceptance_criteria: Vec::new(),
        verification: Vec::new(),
        remaining: vec![actions],
        human_choices: Vec::new(),
    };
    let (worktree, task, documents) = generate::context(repo, ticket, &report);
    generate::run(&worktree, &task, &documents, &report, harness, cancel)
}

#[cfg(test)]
#[path = "attention/legacy_tests.rs"]
mod legacy_tests;
#[cfg(test)]
mod tests;
