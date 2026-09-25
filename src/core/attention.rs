//! Read-only, cached explanations of an external implementation blocker.
mod generate;

use crate::core::implementation::{self, Report};
use crate::harness::{AiHarness, PiHarness};
use serde::{Deserialize, Serialize};
use std::{
    collections::BTreeSet,
    fs,
    path::{Path, PathBuf},
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
        mpsc::{self, Receiver},
    },
};

#[derive(Clone, Debug, PartialEq, Eq, Deserialize, Serialize)]
pub struct OptionBrief {
    pub id: String,
    pub label: String,
    pub meaning: String,
    pub consequence: String,
}

#[derive(Clone, Debug, PartialEq, Eq, Deserialize, Serialize)]
pub struct HumanStep {
    pub owner: String,
    pub action: String,
}

#[derive(Clone, Debug, PartialEq, Eq, Deserialize, Serialize)]
pub struct Brief {
    pub problem: String,
    #[serde(default)]
    pub options: Vec<OptionBrief>,
    #[serde(default)]
    pub steps: Vec<HumanStep>,
    pub after: String,
}

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
}

impl Controller {
    pub fn start(repo: PathBuf, ticket: String, report_path: PathBuf) -> Self {
        let (send, result) = mpsc::channel();
        let cancel = Arc::new(AtomicBool::new(false));
        let worker_cancel = cancel.clone();
        std::thread::spawn(move || {
            let outcome = run(&repo, &ticket, &report_path, &PiHarness, worker_cancel)
                .map_err(|error| format!("{error:#}"));
            let _ = send.send(outcome);
        });
        Self { result, cancel }
    }

    pub fn start_detail(repo: PathBuf, ticket: String, detail: String) -> Self {
        let (send, result) = mpsc::channel();
        let cancel = Arc::new(AtomicBool::new(false));
        let worker_cancel = cancel.clone();
        std::thread::spawn(move || {
            let outcome = run_detail(&repo, &ticket, &detail, &PiHarness, worker_cancel)
                .map_err(|error| format!("{error:#}"));
            let _ = send.send(outcome);
        });
        Self { result, cancel }
    }

    pub fn poll(&self) -> Option<Result<Brief, String>> {
        match self.result.try_recv() {
            Ok(result) => Some(result),
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

fn validate(brief: &Brief, report: &Report) -> anyhow::Result<()> {
    anyhow::ensure!(
        (20..=1200).contains(&brief.problem.trim().chars().count()),
        "Explanation needs a concise, specific problem statement"
    );
    anyhow::ensure!(
        !brief.after.trim().is_empty() && brief.after.chars().count() <= 500,
        "Explanation needs a brief, concrete follow-up"
    );
    for step in &brief.steps {
        anyhow::ensure!(
            !step.owner.trim().is_empty()
                && !step.action.trim().is_empty()
                && step.action.chars().count() <= 500,
            "Explanation has an incomplete human step"
        );
    }
    let mut ids = BTreeSet::new();
    for option in &brief.options {
        anyhow::ensure!(
            !option.id.trim().is_empty()
                && ids.insert(option.id.trim().to_ascii_lowercase())
                && [
                    option.label.as_str(),
                    option.meaning.as_str(),
                    option.consequence.as_str()
                ]
                .iter()
                .all(|part| !part.trim().is_empty() && part.chars().count() <= 600),
            "Explanation has an incomplete or duplicate option"
        );
    }
    let source_ids = explicit_choice_ids(report);
    if !source_ids.is_empty() {
        anyhow::ensure!(
            ids == source_ids,
            "Explanation options do not match the report's choices"
        );
    }
    anyhow::ensure!(
        !brief.options.is_empty() || !brief.steps.is_empty(),
        "Explanation omits every human action"
    );
    Ok(())
}

/// Keep generated buttons tied to IDs explicitly listed in a human choice.
/// IDs are discovered from the report instead of assuming a fixed number of
/// lettered options; ordinary parentheticals outside choice instructions are
/// ignored.
fn explicit_choice_ids(report: &Report) -> BTreeSet<String> {
    let mut ids = BTreeSet::new();
    for line in report.remaining.join("\n").lines() {
        let lower = line.to_ascii_lowercase();
        if ![
            "choose",
            "pick",
            "select",
            "options",
            "alternatives",
            "remedies",
        ]
        .iter()
        .any(|cue| lower.contains(cue))
        {
            continue;
        }
        let mut rest = line;
        while let Some(open) = rest.find('(') {
            rest = &rest[open + 1..];
            let Some(close) = rest.find(')') else {
                break;
            };
            let candidate = rest[..close].trim();
            if !candidate.is_empty()
                && candidate.len() <= 24
                && candidate
                    .bytes()
                    .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'-'))
            {
                ids.insert(candidate.to_ascii_lowercase());
            }
            rest = &rest[close + 1..];
        }
    }
    ids
}

fn run(
    repo: &Path,
    ticket: &str,
    report_path: &Path,
    harness: &dyn AiHarness,
    cancel: Arc<AtomicBool>,
) -> anyhow::Result<Brief> {
    let bytes = fs::read(report_path)?;
    let report: Report = serde_json::from_slice(&bytes)?;
    anyhow::ensure!(report.status == "blocked", "Report is no longer blocked");
    let (worktree, documents) = generate::context(repo, ticket, &report);
    let mut source = bytes;
    source.extend_from_slice(documents.as_bytes());
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
    let brief = generate::run(&worktree, &documents, &report, harness, cancel.clone())?;
    validate(&brief, &report)?;
    anyhow::ensure!(!cancel.load(Ordering::SeqCst), "Explanation cancelled");
    let bytes = serde_json::to_vec_pretty(&Cache {
        source_fingerprint,
        brief: brief.clone(),
    })?;
    let temp = sidecar.with_extension(format!("tmp-{}", std::process::id()));
    fs::write(&temp, bytes)?;
    fs::rename(temp, sidecar)?;
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
        status: "blocked".into(),
        summary: detail.to_owned(),
        acceptance_criteria: Vec::new(),
        verification: Vec::new(),
        remaining: vec![actions],
    };
    let (worktree, documents) = generate::context(repo, ticket, &report);
    generate::run(&worktree, &documents, &report, harness, cancel)
}

#[cfg(test)]
mod tests;
