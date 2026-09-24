//! Resumable ticket implementation. Git worktrees and runtime records are kept
//! independently from planning state; only verified results proceed to a PR.
pub mod cleanup;

use crate::harness::{AiHarness, LiveProgress, PiHarness, PlanningRequest};
use serde::{Deserialize, Serialize};
use std::{
    collections::BTreeSet,
    fs,
    path::{Path, PathBuf},
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
        mpsc::{self, Receiver, Sender},
    },
    time::{Duration, Instant},
};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Implementation {
    pub ticket: String,
    pub ticket_text: String,
    #[serde(default)]
    pub approved_specification: Option<String>,
    #[serde(default)]
    pub approved_product_context: Option<String>,
    #[serde(default)]
    pub completed_dependency_context: Option<String>,
    pub branch: String,
    pub base: String,
    pub base_commit: String,
    pub worktree: PathBuf,
    pub status: String,
    pub detail: String,
    pub pr_url: Option<String>,
    pub verified_head: Option<String>,
    #[serde(default)]
    pub auto_merge: bool,
    #[serde(default)]
    pub merged_commit: Option<String>,
    #[serde(default)]
    pub pr_state: Option<String>,
    #[serde(default)]
    pub pr_checked_at: Option<String>,
    #[serde(default)]
    pub pr_check_attempted_at: Option<String>,
    #[serde(default)]
    pub pr_check_error: Option<String>,
    #[serde(default)]
    pub cleanup: cleanup::Cleanup,
}
#[derive(Debug, Deserialize, Serialize)]
pub(crate) struct Report {
    pub(crate) status: String,
    pub(crate) summary: String,
    pub(crate) acceptance_criteria: Vec<Criterion>,
    pub(crate) verification: Vec<String>,
    pub(crate) remaining: Vec<String>,
}
#[derive(Debug, Deserialize, Serialize)]
pub(crate) struct Criterion {
    pub(crate) criterion: String,
    pub(crate) evidence: String,
}

pub enum Event {
    Progress(LiveProgress),
    Done(Result<Implementation, String>),
}
pub struct Controller {
    #[cfg(test)]
    _keep_alive: Option<Sender<Event>>,
    rx: Receiver<Event>,
    cancel: Arc<AtomicBool>,
}
impl Controller {
    #[cfg(test)]
    pub(crate) fn idle_fixture() -> Self {
        let (tx, rx) = mpsc::channel();
        Self {
            _keep_alive: Some(tx),
            rx,
            cancel: Arc::new(AtomicBool::new(false)),
        }
    }
    #[cfg(test)]
    pub(crate) fn cancellation_requested(&self) -> bool {
        self.cancel.load(Ordering::SeqCst)
    }
    pub fn start(repo: PathBuf, ticket: String, auto_merge: bool) -> Self {
        Self::start_project(repo.clone(), repo, ticket, auto_merge)
    }
    pub fn start_project(
        planning_root: PathBuf,
        target_repo: PathBuf,
        ticket: String,
        auto_merge: bool,
    ) -> Self {
        let (tx, rx) = mpsc::channel();
        let cancel = Arc::new(AtomicBool::new(false));
        let worker_cancel = cancel.clone();
        std::thread::spawn(move || {
            let (progress, updates) = mpsc::channel();
            let fwd = tx.clone();
            let forward = std::thread::spawn(move || {
                for p in updates {
                    let _ = fwd.send(Event::Progress(p));
                }
            });
            let result = run_with_project_options(
                &planning_root,
                &target_repo,
                &ticket,
                &PiHarness,
                worker_cancel,
                progress,
                "gh",
                auto_merge,
            );
            let _ = forward.join();
            let _ = tx.send(Event::Done(result.map_err(|e| format!("{e:#}"))));
        });
        Self { rx, cancel, #[cfg(test)] _keep_alive: None }
    }
    pub fn poll(&self) -> Option<Event> {
        match self.rx.try_recv() {
            Ok(event) => Some(event),
            Err(mpsc::TryRecvError::Empty) => None,
            Err(mpsc::TryRecvError::Disconnected) => Some(Event::Done(Err(
                "Implementation worker stopped without a result. Work is preserved; inspect the task failure and Resume implementation.".into(),
            ))),
        }
    }
    pub fn request_cancel(&self) {
        self.cancel.store(true, Ordering::SeqCst);
    }
}
impl Drop for Controller {
    fn drop(&mut self) {
        self.request_cancel();
    }
}

struct Runner {
    gh: String,
    deadline: Instant,
    cancel: Arc<AtomicBool>,
    progress: Sender<LiveProgress>,
}
impl Runner {
    fn remaining(&self) -> anyhow::Result<Duration> {
        anyhow::ensure!(
            !self.cancel.load(Ordering::SeqCst),
            "Implementation cancelled. The worktree is preserved; choose Resume implementation to continue."
        );
        self.deadline
            .checked_duration_since(Instant::now())
            .ok_or_else(|| {
                anyhow::anyhow!(
                    "Implementation budget expired. The worktree is preserved for resume."
                )
            })
    }
    fn update(&self, text: impl Into<String>) {
        let _ = self.progress.send(LiveProgress {
            activity: Some(text.into()),
            ..Default::default()
        });
    }
    fn command(&self, cwd: &Path, program: &str, args: &[&str]) -> anyhow::Result<String> {
        self.remaining()?;
        let argv = std::iter::once(program.to_owned())
            .chain(args.iter().map(|s| s.to_string()))
            .collect::<Vec<_>>();
        let child = crate::harness::pi_proc::spawn(&argv, cwd)?;
        let mut output = String::new();
        let mut error = String::new();
        loop {
            if let Err(e) = self.remaining() {
                child.kill();
                anyhow::bail!("{e}\nCommand: {program} {}\nWorking directory: {}\nstdout:\n{output}\nstderr:\n{error}", args.join(" "), cwd.display());
            }
            use crate::harness::pi_proc::{PollState, StreamEvt};
            match child.poll_next(Duration::from_millis(100)) {
                Ok(StreamEvt::Stdout(line)) => append_tail(&mut output, &line),
                Ok(StreamEvt::Stderr(line)) => append_tail(&mut error, &line),
                Ok(StreamEvt::Exited(ok)) => {
                    anyhow::ensure!(ok, "{program} failed:\nstdout:\n{output}\nstderr:\n{error}");
                    return Ok(output.trim().into());
                }
                Err(PollState::Closed) => anyhow::bail!("{program} closed without an exit result"),
                Err(PollState::Pending) => {}
            }
        }
    }
    fn check_storage(&self, cwd: &Path) -> anyhow::Result<()> {
        #[cfg(unix)]
        {
            let existing = cwd.ancestors().find(|path| path.is_dir())
                .ok_or_else(|| anyhow::anyhow!("Cannot locate filesystem for {}", cwd.display()))?;
            let output = self.command(existing, "df", &["-Pk", "."])?;
            let available = output.lines().last()
                .and_then(|line| line.split_whitespace().nth(3))
                .and_then(|value| value.parse::<u64>().ok())
                .ok_or_else(|| anyhow::anyhow!("Cannot determine available disk space for {}", cwd.display()))?;
            anyhow::ensure!(available >= 1024 * 1024,
                "Insufficient disk space at {}: {} MiB available; at least 1 GiB is required to start implementation or verification. Free rebuildable build caches, then Resume implementation. Existing work is preserved.", cwd.display(), available / 1024);
        }
        Ok(())
    }
    fn verify(&self, cwd: &Path, command: &str) -> anyhow::Result<String> {
        self.check_storage(cwd)?;
        let path = cwd
            .to_str()
            .ok_or_else(|| anyhow::anyhow!("Non-UTF8 worktree path"))?;
        // Pass the path as data, never interpolate it into shell source.
        let script = format!("PACKET_WORKTREE=$1; export PACKET_WORKTREE\n{command}");
        self.command(
            cwd,
            "/bin/sh",
            &["-c", &script, "packet-verification", path],
        )
    }
    fn git(&self, cwd: &Path, args: &[&str]) -> anyhow::Result<String> {
        let retries = if matches!(args.first(), Some(&"fetch" | &"ls-remote")) {
            3
        } else {
            1
        };
        let mut last = None;
        for attempt in 0..retries {
            match self.command(cwd, "git", args) {
                Ok(output) => return Ok(output),
                Err(error) => {
                    self.remaining()?;
                    last = Some(error);
                }
            }
            if attempt + 1 < retries {
                self.update("Retrying temporary Git connection failure…");
            }
        }
        Err(last.unwrap())
    }
}
fn append_tail(out: &mut String, line: &str) {
    out.push_str(line);
    out.push('\n');
    if out.len() > 32_000 {
        let boundary = out
            .char_indices()
            .map(|(i, _)| i)
            .find(|i| *i >= out.len() - 24_000)
            .unwrap_or(0);
        out.drain(..boundary);
    }
}
fn key(ticket: &str) -> String {
    format!(
        "{}-{:016x}",
        crate::artifacts::task_docs::slug(
            Path::new(ticket)
                .file_stem()
                .and_then(|s| s.to_str())
                .unwrap_or("ticket")
        ),
        crate::persistence::fnv1a64(ticket.as_bytes())
    )
}
fn common(repo: &Path) -> anyhow::Result<PathBuf> {
    let output = std::process::Command::new("git")
        .args(["rev-parse", "--path-format=absolute", "--git-common-dir"])
        .current_dir(repo)
        .output()?;
    anyhow::ensure!(output.status.success(), "Cannot locate Git metadata");
    Ok(PathBuf::from(String::from_utf8(output.stdout)?.trim()))
}
fn state_dir(repo: &Path, ticket: &str) -> anyhow::Result<PathBuf> {
    // State belongs to the repository's Packet workspace, not to .git. This
    // keeps resumable implementation evidence visible, portable, and backed
    // up with the rest of the Packet artifacts.
    Ok(record_directory(&repo.join(crate::artifacts::packet::PACKET_IMPLEMENTATION_DIR), repo, ticket))
}

fn legacy_state_dir(repo: &Path, ticket: &str) -> anyhow::Result<PathBuf> {
    Ok(record_directory(&common(repo)?.join("packet-implementations"), repo, ticket))
}

// A directory move changes board paths, not implementation/worktree identity.
// Only adopt the old record when its frozen ticket still matches exactly.
fn record_directory(root: &Path, repo: &Path, ticket: &str) -> PathBuf {
    let direct = root.join(key(ticket));
    if direct.join("state.json").exists() { return direct; }
    if let Some(old) = ticket.strip_prefix(".kool-ade-packet/") {
        if old.starts_with("planning/tasks/") && !repo.join(old).exists() {
            let candidate = root.join(key(old));
            if let Ok(bytes) = fs::read(candidate.join("state.json")) {
                if let Ok(record) = serde_json::from_slice::<Implementation>(&bytes) {
                    if (record.ticket == old || record.ticket == ticket)
                        && fs::read_to_string(repo.join(ticket))
                            .is_ok_and(|text| text == record.ticket_text) {
                        return candidate;
                    }
                }
            }
        }
    }
    direct
}

fn board_ticket(repo: &Path, state: &Implementation) -> String {
    if state.ticket.starts_with("planning/tasks/") && !repo.join(&state.ticket).exists() {
        let relocated = format!(".kool-ade-packet/{}", state.ticket);
        if fs::read_to_string(repo.join(&relocated)).is_ok_and(|text| text == state.ticket_text) {
            return relocated;
        }
    }
    state.ticket.clone()
}

/// Keep original evidence identities while indexing state by current board paths.
pub fn load_board_states(repo: &Path) -> std::collections::BTreeMap<String, Implementation> {
    let mut result = std::collections::BTreeMap::new();
    for state in load_all(repo) {
        let ticket = board_ticket(repo, &state);
        // An explicit record for the current path wins over a historical alias.
        if !result.contains_key(&ticket)
            || (ticket == state.ticket
                && result.get(&ticket).is_some_and(|previous: &Implementation| previous.ticket != ticket)) {
            result.insert(ticket, state);
        }
    }
    result
}

fn migrate_legacy_state(repo: &Path, ticket: &str) -> anyhow::Result<()> {
    let target = state_dir(repo, ticket)?;
    let legacy = legacy_state_dir(repo, ticket)?;
    if !target.exists() && legacy.exists() {
        if let Some(parent) = target.parent() {
            fs::create_dir_all(parent)?;
        }
        fs::rename(legacy, target)?;
    }
    Ok(())
}
pub fn load_all(repo: &Path) -> Vec<Implementation> {
    let mut records = Vec::new();
    for root in [
        repo.join(crate::artifacts::packet::PACKET_IMPLEMENTATION_DIR),
        common(repo)
            .ok()
            .map(|p| p.join("packet-implementations"))
            .unwrap_or_default(),
    ] {
        let Ok(entries) = fs::read_dir(root) else {
            continue;
        };
        records.extend(entries.filter_map(Result::ok).filter_map(|entry| {
            serde_json::from_slice(&fs::read(entry.path().join("state.json")).ok()?).ok()
        }));
    }
    records
}
pub fn load(repo: &Path, ticket: &str) -> Option<Implementation> {
    let path = state_dir(repo, ticket).ok()?.join("state.json");
    let path = if path.exists() {
        path
    } else {
        legacy_state_dir(repo, ticket).ok()?.join("state.json")
    };
    serde_json::from_slice(&fs::read(path).ok()?).ok()
}
fn save(dir: &Path, state: &Implementation) -> anyhow::Result<()> {
    let temporary = dir.join("state.json.tmp");
    fs::write(&temporary, serde_json::to_vec_pretty(state)?)?;
    fs::rename(temporary, dir.join("state.json"))?;
    Ok(())
}

/// Refresh in a worker: GitHub outages must not block the UI or erase the
/// last confirmed state. The implementation lock prevents stale writes.
pub struct PrRefresh {
    rx: Receiver<Vec<(String, String)>>,
    cancel: Arc<AtomicBool>,
}
impl PrRefresh {
    pub fn start(repo: PathBuf, tickets: Vec<String>) -> Self {
        let (tx, rx) = mpsc::channel();
        let cancel = Arc::new(AtomicBool::new(false));
        let worker_cancel = cancel.clone();
        std::thread::spawn(move || {
            let (progress, _updates) = mpsc::channel();
            let runner = Runner {
                gh: "gh".into(),
                deadline: Instant::now() + Duration::from_secs(45),
                cancel: worker_cancel,
                progress,
            };
            let mut errors = Vec::new();
            for ticket in tickets {
                if runner.remaining().is_err() {
                    break;
                }
                if let Err(error) = refresh_pr(&repo, &ticket, &runner) {
                    errors.push((ticket, format!("{error:#}")));
                }
            }
            let _ = tx.send(errors);
        });
        Self { rx, cancel }
    }
    pub fn poll(&self) -> Option<Vec<(String, String)>> {
        match self.rx.try_recv() {
            Ok(errors) => Some(errors),
            Err(mpsc::TryRecvError::Empty) => None,
            Err(mpsc::TryRecvError::Disconnected) => Some(vec![(String::new(), "Task maintenance worker stopped unexpectedly; retrying on the next refresh".into())]),
        }
    }
}
impl Drop for PrRefresh {
    fn drop(&mut self) {
        self.cancel.store(true, Ordering::SeqCst);
    }
}

fn refresh_pr(repo: &Path, ticket: &str, runner: &Runner) -> anyhow::Result<()> {
    let current = state_dir(repo, ticket)?;
    let dir = if current.join("state.json").exists() { current } else { legacy_state_dir(repo, ticket)? };
    let lock = fs::OpenOptions::new()
        .read(true)
        .write(true)
        .create(true)
        .truncate(false)
        .open(dir.join("run.lock"))?;
    if lock.try_lock().is_err() {
        // A concurrent implementation can hold this lock for minutes, so never
        // block. Brief platform stalls have, however, been observed to stretch
        // short critical sections past a single immediate attempt; give the
        // holder a bounded moment to finish before falling back to the
        // deliberate no-op.
        for delay_ms in [10u64, 20, 20, 20] {
            std::thread::sleep(std::time::Duration::from_millis(delay_ms));
            if lock.try_lock().is_ok() {
                break;
            }
        }
        if lock.try_lock().is_err() {
            return Ok(());
        }
    }
    let mut state: Implementation = serde_json::from_slice(&fs::read(dir.join("state.json"))?)?;
    let target_repo = target_repository(repo, ticket)?;
    if state.status == "Done" && (state.merged_commit.is_some() || state.pr_url.is_none()) {
        cleanup::run(&target_repo, &dir, &mut state, runner);
        return save(&dir, &state);
    }
    let Some(url) = state.pr_url.clone() else {
        return Ok(());
    };
    if state.pr_state.as_deref() == Some("MERGED") && state.merged_commit.is_some() {
        return Ok(());
    }
    state.pr_check_attempted_at = Some(chrono::Utc::now().to_rfc3339());
    let result = (|| -> anyhow::Result<(String, Option<String>)> {
        let output = runner.command(
            &target_repo,
            &runner.gh,
            &["pr", "view", &url, "--json", "state,mergeCommit"],
        )?;
        let value: serde_json::Value = serde_json::from_str(&output)?;
        let status = value["state"].as_str().unwrap_or_default();
        anyhow::ensure!(
            matches!(status, "OPEN" | "CLOSED" | "MERGED"),
            "GitHub returned an unknown PR state"
        );
        let merged = value["mergeCommit"]["oid"]
            .as_str()
            .filter(|oid| !oid.is_empty())
            .map(str::to_owned);
        Ok((status.to_owned(), merged))
    })();
    match result {
        Ok((status, merged)) => {
            state.status = match status.as_str() {
                "MERGED" => "Done",
                "CLOSED" => "PR closed",
                _ => "PR created",
            }
            .into();
            state.pr_state = Some(status);
            if merged.is_some() {
                state.merged_commit = merged;
            }
            state.pr_checked_at = Some(chrono::Utc::now().to_rfc3339());
            state.pr_check_error = None;
        }
        Err(error) => state.pr_check_error = Some(error.to_string()),
    }
    cleanup::run(&target_repo, &dir, &mut state, runner);
    save(&dir, &state)
}

pub const BOARD_COLUMNS: [&str; 5] = [
    "To do",
    "In progress",
    "In review",
    "Needs attention",
    "Done",
];
pub fn board_column(state: Option<&Implementation>, busy: bool) -> usize {
    let Some(state) = state else {
        return if busy { 1 } else { 0 };
    };
    match state.pr_state.as_deref() {
        Some("MERGED") => return 4,
        Some("CLOSED") => return 3,
        _ => {}
    }
    match state.status.as_str() {
        "Done" => 4,
        "PR created" => 2,
        "Preparing" | "Implementing" | "Verifying" | "Ready for PR" | "Publishing"
        | "Waiting to merge"
            if busy =>
        {
            1
        }
        _ => 3,
    }
}
fn resume_failure_context(detail: &str) -> String {
    // Also unwrap legacy errors whose complete correction histories were nested
    // on every resume. Keep only the newest diagnostic in the active prompt.
    let start = detail.rfind("\nAttempt ").into_iter()
        .chain(detail.rfind("\nHarness failure ")).max();
    let latest = start.map(|index| &detail[index + 1..]).unwrap_or(detail);
    let latest = latest.rsplit_once("Latest failure: ").map(|(_, tail)| tail)
        .unwrap_or(latest);
    let latest = latest.split("\nAUTOMATIC BLOCKER RECOVERY REQUIRED").next().unwrap_or(latest);
    let latest = latest.split("\nSELF-REPAIR REQUIRED").next().unwrap_or(latest);
    crate::core::context_build::clip(latest, 4000)
}

fn read_ticket(repo: &Path, ticket: &str) -> anyhow::Result<String> {
    anyhow::ensure!(
        (ticket.starts_with("planning/tasks/")
            || ticket.starts_with(".kool-ade-packet/planning/tasks/"))
            && Path::new(ticket)
                .file_name()
                .and_then(|n| n.to_str())
                .is_some_and(crate::artifacts::task_docs::is_task_story_filename),
        "Select a generated task story (numbered or feature-ID filename)"
    );
    anyhow::ensure!(
        Path::new(ticket)
            .components()
            .all(|c| matches!(c, std::path::Component::Normal(_))),
        "Invalid ticket path"
    );
    let path = repo.join(ticket).canonicalize()?;
    anyhow::ensure!(
        path.starts_with(repo.canonicalize()?),
        "Ticket is outside the repository"
    );
    let text = fs::read_to_string(path)?;
    anyhow::ensure!(!text.trim().is_empty(), "Ticket is empty");
    Ok(text)
}

pub fn target_repository(planning_root: &Path, ticket: &str) -> anyhow::Result<PathBuf> {
    let text = read_ticket(planning_root, ticket)?;
    let manifest = crate::core::project_repos::ProjectManifest::load(planning_root)?;
    let id = text
        .lines()
        .find_map(|line| line.strip_prefix("Repository: "));
    anyhow::ensure!(
        id.is_some() || manifest.repositories.len() == 1,
        "Multi-repository task lacks a repository target"
    );
    manifest.target(planning_root, id.unwrap_or("root").trim())
}

pub fn run(
    repo: &Path,
    ticket: &str,
    harness: &dyn AiHarness,
    cancel: Arc<AtomicBool>,
    progress: Sender<LiveProgress>,
) -> anyhow::Result<Implementation> {
    run_with_options(repo, ticket, harness, cancel, progress, "gh", true)
}
#[cfg(test)]
fn run_with_gh(
    repo: &Path,
    ticket: &str,
    harness: &dyn AiHarness,
    cancel: Arc<AtomicBool>,
    progress: Sender<LiveProgress>,
    gh: &str,
) -> anyhow::Result<Implementation> {
    run_with_options(repo, ticket, harness, cancel, progress, gh, false)
}
fn run_with_options(
    repo: &Path,
    ticket: &str,
    harness: &dyn AiHarness,
    cancel: Arc<AtomicBool>,
    progress: Sender<LiveProgress>,
    gh: &str,
    auto_merge: bool,
) -> anyhow::Result<Implementation> {
    run_with_project_options(
        repo, repo, ticket, harness, cancel, progress, gh, auto_merge,
    )
}
fn run_with_project_options(
    planning_root: &Path,
    repo: &Path,
    ticket: &str,
    harness: &dyn AiHarness,
    cancel: Arc<AtomicBool>,
    progress: Sender<LiveProgress>,
    gh: &str,
    auto_merge: bool,
) -> anyhow::Result<Implementation> {
    anyhow::ensure!(
        target_repository(planning_root, ticket)?.canonicalize()? == repo.canonicalize()?,
        "Implementation checkout does not match the task repository manifest"
    );
    let runner = Runner {
        gh: gh.into(),
        deadline: Instant::now() + crate::core::turn::configured_turn_timeout(),
        cancel,
        progress,
    };
    let text = read_ticket(planning_root, ticket)?;
    migrate_legacy_state(planning_root, ticket)?;
    let dir = state_dir(planning_root, ticket)?;
    fs::create_dir_all(&dir)?;
    let lock = fs::OpenOptions::new()
        .read(true)
        .write(true)
        .create(true)
        .truncate(false)
        .open(dir.join("run.lock"))?;
    lock.try_lock().map_err(|_| {
        anyhow::anyhow!("This ticket is already being implemented in another Packet instance")
    })?;
    let mut state = if dir.join("state.json").exists() {
        let state: Implementation = serde_json::from_slice(&fs::read(dir.join("state.json"))?)?;
        anyhow::ensure!(
            (state.ticket == ticket || board_ticket(planning_root, &state) == ticket)
                && state.ticket_text == text,
            "Ticket changed since implementation started. Review the existing worktree before starting a revised ticket."
        );
        state
    } else {
        // Persist identity before worktree creation so crashes can be resumed.
        let base = if auto_merge {
            default_branch(repo, &runner)?
        } else {
            runner.git(repo, &["symbolic-ref", "--short", "HEAD"])?
        };
        runner.update(format!("Fetching latest origin/{base}…"));
        // Fetch an explicit branch and resolve its immutable commit. Do not pull
        // into the user's checkout, which may contain unrelated drafts.
        let remote_ref = format!("refs/packet-bases/{}", key(ticket));
        runner.git(
            repo,
            &[
                "fetch",
                "--no-tags",
                "--no-write-fetch-head",
                "origin",
                &format!("+refs/heads/{base}:{remote_ref}"),
            ],
        )?;
        let local = runner.git(repo, &["rev-parse", "HEAD"])?;
        let remote = runner.git(repo, &["rev-parse", &remote_ref])?;
        let head = if runner
            .git(repo, &["merge-base", "--is-ancestor", &local, &remote])
            .is_ok()
        {
            remote
        } else if auto_merge
            && runner
                .git(repo, &["merge-base", "--is-ancestor", &remote, &local])
                .is_err()
        {
            // Auto workers start from current remote truth, leaving divergent
            // local development history intact in the operator's checkout.
            remote
        } else {
            runner.git(repo, &["merge-base", "--is-ancestor", &remote, &local])
                .map_err(|_| anyhow::anyhow!("Local {base} and freshly fetched origin/{base} have diverged. Reconcile the branch before implementing; no work was discarded."))?;
            local
        };
        let root = repo
            .parent()
            .ok_or_else(|| anyhow::anyhow!("Repository has no parent"))?
            .join(".packet-worktrees")
            .join(crate::persistence::project_slug(&repo.canonicalize()?));
        fs::create_dir_all(&root)?;
        let dependency_context = completed_dependency_context(planning_root, ticket, &text)?;
        Implementation {
            ticket: ticket.into(),
            ticket_text: text,
            approved_specification: Path::new(ticket).parent().and_then(|parent| {
                fs::read_to_string(planning_root.join(parent).join("specification.md")).ok()
            }),
            approved_product_context: scoped_product_context(planning_root, ticket)?,
            completed_dependency_context: dependency_context,
            branch: format!("packet/{}", key(ticket)),
            base,
            base_commit: head,
            worktree: root.join(key(ticket)),
            status: "Preparing".into(),
            detail: String::new(),
            pr_url: None,
            verified_head: None,
            auto_merge,
            merged_commit: None,
            pr_state: None,
            pr_checked_at: None,
            pr_check_attempted_at: None,
            pr_check_error: None,
            cleanup: Default::default(),
        }
    };
    if state.pr_url.is_none() && state.merged_commit.is_none() {
        state.auto_merge = auto_merge;
    }
    if state.status == "Done" {
        return Ok(state);
    }
    save(&dir, &state)?;
    let result = runner.check_storage(&dir)
        .and_then(|_| runner.check_storage(&state.worktree))
        .and_then(|_| execute(planning_root, repo, &dir, &mut state, harness, &runner));
    if let Err(error) = result {
        state.status = if runner.cancel.load(Ordering::SeqCst) {
            "Interrupted"
        } else {
            "Needs attention"
        }
        .into();
        state.detail = format!("{error:#}");
        if let Err(save_error) = save(&dir, &state) {
            anyhow::bail!("{}\nCould not persist the failed task state at {}: {save_error:#}. Check available disk space and permissions, then Resume implementation.", state.detail, dir.display());
        }
        return Err(error);
    }
    // Completion is already durable. Reclamation has its own bounded budget
    // and records failure without turning a published task back into a failure.
    let cleanup_runner = Runner {
        gh: runner.gh.clone(), deadline: Instant::now() + Duration::from_secs(120),
        cancel: runner.cancel.clone(), progress: runner.progress.clone(),
    };
    cleanup::run(repo, &dir, &mut state, &cleanup_runner);
    if let Err(error) = save(&dir, &state) {
        if state.status != "Done" { return Err(error); }
        state.cleanup.error = Some(format!("Could not save cleanup outcome: {error:#}. {}", state.cleanup.error.as_deref().unwrap_or("Cleanup will be checked again on the next refresh.")));
    }
    Ok(state)
}

fn scoped_product_context(planning_root: &Path, ticket: &str) -> anyhow::Result<Option<String>> {
    let Some(parent) = Path::new(ticket).parent() else {
        return Ok(None);
    };
    let contract_path = planning_root.join(parent).join("contract.json");
    let bytes = match fs::read(&contract_path) {
        Ok(bytes) => bytes,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(error) => return Err(error.into()),
    };
    let contract: crate::core::contract_snapshot::BatchContract = serde_json::from_slice(&bytes)?;
    let mut context = String::new();
    for (id, body) in contract.product_modules {
        context.push_str(&format!("\n=== product:{id} ===\n{body}\n"));
    }
    Ok(Some(context))
}

/// Collect objective history evidence for explicit commit references in a
/// ticket before a worker starts. Exact-footprint requirements often name an
/// older checkpoint; this makes the changes already present at the task base
/// visible and reviewable before implementation or recovery begins.
fn history_preflight_context(
    runner: &Runner,
    worktree: &Path,
    base_commit: &str,
    ticket_text: &str,
) -> anyhow::Result<String> {
    let anchors = ticket_text
        .split(|c: char| !c.is_ascii_hexdigit())
        .filter(|token| (7..=40).contains(&token.len()))
        .map(str::to_ascii_lowercase)
        .collect::<BTreeSet<_>>();
    let mut evidence = format!(
        "Task base: {base_commit}\nTicket commit references: {}\n",
        anchors.len()
    );
    for anchor in anchors.iter().take(20) {
        let Ok(resolved) = runner.git(
            worktree,
            &["rev-parse", "--verify", &format!("{anchor}^{{commit}}")],
        ) else {
            evidence.push_str(&format!("\n{anchor}: not a resolvable commit in this checkout\n"));
            continue;
        };
        let common = runner.git(worktree, &["merge-base", &resolved, base_commit]);
        match common {
            Ok(common) if common == resolved => {
                let paths = runner.git(worktree, &["diff", "--name-status", &resolved, base_commit])?;
                evidence.push_str(&format!(
                    "\n{anchor} resolves to {resolved} and is an ancestor of the task base.\nChanged paths from that checkpoint to the task base (git diff --name-status):\n{}\n",
                    if paths.is_empty() { "(none)" } else { paths.as_str() }
                ));
            }
            Ok(common) => evidence.push_str(&format!(
                "\n{anchor} resolves to {resolved}, but is not an ancestor of the task base (merge base {common}).\n"
            )),
            Err(_) => evidence.push_str(&format!(
                "\n{anchor} resolves to {resolved}, but shares no reachable history with the task base.\n"
            )),
        }
    }
    if anchors.len() > 20 {
        evidence.push_str(&format!(
            "\nOnly the first 20 of {} ticket references are shown.\n",
            anchors.len()
        ));
    }
    Ok(evidence)
}

pub fn completed_dependency_context(
    planning_root: &Path,
    ticket: &str,
    ticket_text: &str,
) -> anyhow::Result<Option<String>> {
    use pulldown_cmark::{Event, Parser, Tag};
    let mut in_dependencies = false;
    let mut section = String::new();
    for line in ticket_text.lines() {
        if line.starts_with("## ") {
            in_dependencies = line.trim().eq_ignore_ascii_case("## Dependencies");
            continue;
        }
        if in_dependencies {
            section.push_str(line);
            section.push('\n');
        }
    }
    let mut links = std::collections::BTreeSet::new();
    for event in Parser::new(&section) {
        if let Event::Start(Tag::Link { dest_url, .. }) = event {
            let filename = Path::new(dest_url.as_ref());
            anyhow::ensure!(
                filename.components().count() == 1 && dest_url.ends_with(".md"),
                "Invalid dependency link {dest_url}"
            );
            links.insert(dest_url.to_string());
        }
    }
    anyhow::ensure!(links.len() <= 16, "Too many task dependencies");
    let mut context = String::new();
    for filename in links {
        let relative = Path::new(ticket)
            .parent()
            .ok_or_else(|| anyhow::anyhow!("Task path has no batch directory"))?
            .join(filename);
        let relative = relative.to_string_lossy();
        anyhow::ensure!(relative.as_ref() != ticket, "Task cannot depend on itself");
        let record = load(planning_root, &relative)
            .ok_or_else(|| anyhow::anyhow!("Dependency {relative} has no implementation record"))?;
        anyhow::ensure!(
            (record.status == "Done" || record.pr_state.as_deref() == Some("MERGED"))
                && record.merged_commit.is_some(),
            "Dependency {relative} has not merged"
        );
        let story = fs::read_to_string(planning_root.join(relative.as_ref()))?;
        anyhow::ensure!(
            story == record.ticket_text,
            "Dependency {relative} story changed"
        );
        context.push_str(&format!(
            "\n=== Completed dependency {relative} ===\nMerged commit: {}\n{}\n",
            record.merged_commit.as_deref().unwrap(),
            story.chars().take(6_000).collect::<String>()
        ));
    }
    Ok((!context.is_empty()).then_some(context))
}

fn prepare_verified(
    repo: &Path,
    dir: &Path,
    state: &mut Implementation,
    harness: &dyn AiHarness,
    runner: &Runner,
) -> anyhow::Result<()> {
    runner.update("Preparing implementation worktree…");
    if state.worktree.exists() {
        anyhow::ensure!(
            common(&state.worktree)?.canonicalize()? == common(repo)?.canonicalize()?,
            "Existing worktree belongs to a different repository; no changes made"
        );
        anyhow::ensure!(
            runner.git(&state.worktree, &["symbolic-ref", "--short", "HEAD"])? == state.branch,
            "Existing worktree is on another branch; no changes made"
        );
    } else {
        let path = state
            .worktree
            .to_str()
            .ok_or_else(|| anyhow::anyhow!("Non-UTF8 worktree path"))?;
        let branch_exists = runner
            .git(
                repo,
                &[
                    "show-ref",
                    "--verify",
                    &format!("refs/heads/{}", state.branch),
                ],
            )
            .is_ok();
        if branch_exists {
            runner.git(repo, &["worktree", "add", path, &state.branch])?;
        } else {
            runner.git(
                repo,
                &[
                    "worktree",
                    "add",
                    "-b",
                    &state.branch,
                    path,
                    &state.base_commit,
                ],
            )?;
        }
    }
    let clean = runner
        .git(&state.worktree, &["status", "--porcelain"])?
        .is_empty();
    let head = runner.git(&state.worktree, &["rev-parse", "HEAD"])?;
    let already_verified = clean
        && state.verified_head.as_deref() == Some(head.as_str())
        && (!state.auto_merge || dir.join("verified-report.json").exists());
    if already_verified && state.pr_url.is_some() {
        return Ok(());
    }
    if !already_verified {
        // Report and verification corrections each have a limit of three; all share the original deadline.
        // Prior-run evidence is context, never part of this run's retry accounting.
        let prior_detail = state.detail.clone();
        if !prior_detail.is_empty() {
            fs::write(dir.join(format!("{}-resume-context.txt",
                chrono::Utc::now().timestamp_nanos_opt().unwrap_or_default())), &prior_detail)?;
        }
        let prior_failure = resume_failure_context(&prior_detail);
        let mut feedback = String::new();
        state.detail = "Starting a fresh attempt budget; previous work and evidence are preserved.".into();
        let mut previous_response = String::new();
        let specification = state
            .approved_specification
            .clone()
            .or_else(|| {
                Path::new(&state.ticket)
                    .parent()
                    .map(|p| repo.join(p).join("specification.md"))
                    .and_then(|p| fs::read_to_string(p).ok())
            })
            .unwrap_or_default();
        let mut attempt = 0;
        let mut report_corrections = 0;
        let mut verification_corrections = 0;
        let mut harness_failures = 0;
        let mut healing_attempts = 0;
        let report = loop {
            runner.remaining()?;
            attempt += 1;
            anyhow::ensure!(
                common(&state.worktree)?.canonicalize()? == common(repo)?.canonicalize()?,
                "Implementation worktree belongs to a different repository"
            );
            anyhow::ensure!(
                runner.git(&state.worktree, &["symbolic-ref", "--short", "HEAD"])? == state.branch,
                "Implementation changed branches; refusing to continue"
            );
            state.status = "Implementing".into();
            save(dir, state)?;
            runner.update(format!(
                "Implementing {} (attempt {attempt})…",
                state.ticket
            ));
            let status = runner.git(&state.worktree, &["status", "--short"])?;
            let log = runner.git(&state.worktree, &["log", "-5", "--oneline"])?;
            let stamp = chrono::Utc::now().timestamp_nanos_opt().unwrap_or_default();
            let history_evidence = history_preflight_context(
                runner,
                &state.worktree,
                &state.base_commit,
                &state.ticket_text,
            )?;
            let history_evidence_path = dir.join(format!("{stamp}-history-preflight.txt"));
            fs::write(&history_evidence_path, &history_evidence)?;
            let mut prompt = format!(
                "Implement this ticket in the CURRENT working directory, a dedicated Git worktree. This may be a RESUME: inspect git status, existing diffs, commits, untracked files, tests and repository instructions FIRST. Preserve and complete existing work; do not restart, reset, clean, discard or overwrite unrelated changes. Verify prerequisites and dependencies; report blocked if unavailable. Implement only this ticket's scope. Run the required checks and repair failures. Do not change branches, create worktrees, commit, push, create PRs or merge; Packet owns those steps. Do not modify the original checkout.\n\nTICKET PATH: {}\nTICKET CONTENT:\n{}\n\nAPPROVED SPECIFICATION:\n{}\n\nAFFECTED PRODUCT MODULES (FROZEN AT TASK APPROVAL):\n{}\n\nCURRENT STATUS:\n{}\nRECENT COMMITS:\n{}\n\nReturn a complete JSON object with status (complete or blocked), summary, acceptance_criteria (array of objects with criterion copied verbatim from the ticket and concrete evidence), verification (array of runnable POSIX /bin/sh commands; each runs in a NEW shell starting in this worktree, with PACKET_WORKTREE set to its absolute path; no shell variables or cwd changes carry between commands), remaining (array of unresolved work). Complete requires every ticket criterion met, meaningful checks passing, and remaining empty. Use actual commands without placeholder paths. Before changing directories, capture paths or use \"$PACKET_WORKTREE/Cargo.toml\"; $(pwd) after cd refers to the NEW directory. Do not use Bash-only syntax. When testing commands yourself, export PACKET_WORKTREE to this worktree path before invoking /bin/sh. Execute exactly the commands you report using /bin/sh. Assert expected outcomes and preserve command exit failures: capture output to a file, then check it, rather than masking a failed command with a successful pipeline or command substitution. Never claim success from an exit code alone or invent results. Do not include prose outside the JSON.",
                state.ticket,
                state.ticket_text,
                specification,
                state
                    .approved_product_context
                    .as_deref()
                    .unwrap_or("Legacy task: no scoped product snapshot."),
                status,
                log
            );
            prompt.insert_str(0, "FEASIBILITY PREFLIGHT — before edits or expensive checks, compare every requirement about an exact file list, commit footprint, history, or frozen baseline against the actual base commit and reachable history. If a requirement is already impossible because published commits contain forbidden changes, or would require rewriting history or out-of-scope files, stop and report blocked before implementation. Name the exact conflicting requirement, show the smallest concrete evidence, and make the remaining item a decision for the person who owns the contract (for example: approve the realized footprint, revise the predicate, or authorize history repair). Do not spend recovery turns repeating checks that cannot change this fact. Distinguish this from a code or test defect that can be repaired in this worktree.\n\n");
            prompt.push_str(&format!(
                "\n\nMECHANICALLY COLLECTED HISTORY PREFLIGHT (also saved at {}):\n{}\nCompare any ticket-stated exact footprint with these reachable-history facts before editing. Explicitly say whether the expected table describes cumulative feature history or this ticket's changes from its task base.\n",
                history_evidence_path.display(), history_evidence
            ));
            prompt.push_str("Write summary for an operator: lead with the outcome in plain language, then state the next step as an action with its owner. Define uncommon gate jargon on first use. Make each remaining entry start with the responsible person or role and a verb (for example, `Adjudicator: approve ...` or `Operator: run ...`); name the exact artifact or command and expected result.\n");
            if let Some(dependencies) = &state.completed_dependency_context {
                prompt.push_str(&format!(
                    "\n\nCOMPLETED DEPENDENCY CONTRACTS:\n{dependencies}"
                ));
            }

            if !prior_failure.is_empty() {
                prompt.push_str(&format!("\n\nPREVIOUS STOP / CORRECTION REQUIRED (prior run, context only):\n{prior_failure}\nThis run has a fresh report, verification, harness, and self-repair budget. Prior attempts do not consume it. Preserve previous work; do not treat previous retry exhaustion as a current blocker. Actual unmet prerequisites and acceptance checks still apply.\n"));
            }
            if !feedback.is_empty() {
                prompt.push_str(&format!("\n\nPREVIOUS STOP / CORRECTION REQUIRED:\n{feedback}\nContinue in this same worktree. Treat this as a correction history: keep earlier fixes and address the newest failure without reintroducing older ones. Inspect and preserve existing work. Correct the report or implementation and rerun affected checks. Copy acceptance criterion text EXACTLY, including any spelling mistakes; do not edit the ticket to satisfy this check. Return the full JSON report, not just the correction. Do not weaken or bypass failing checks. Before repeating recovery, check whether the failure is a fixed contradiction in the frozen base/history; if so, preserve the evidence and report the exact human decision needed instead of repeating machine checks. Report blocked for prerequisites or decisions that require human intervention.\nPrevious response (possibly truncated):\n{previous_response}"));
            }
            let report_path = dir.join(format!("{stamp}-report.json"));
            prompt.push_str(&format!("\n\nRECOVERY REPORT FILE: {}\nAfter verification, atomically write the same complete JSON report to this absolute file (temporary sibling then rename) before your final response. This preserves completion if the CLI loses its final message.\nYou may fix the root cause of encountered failures and add regression coverage in this worktree when necessary. Keep repairs focused, preserve checks, and do not commit them yourself: Packet verifies and commits the task and its recovery fixes together atomically.\n", report_path.display()));
            let request = PlanningRequest { implementation: true, read_only: false, reasoning_level: "medium".into(), repo_root: state.worktree.clone(), prompt_body: prompt, system_instructions: "You are an implementation agent. Read and follow repository AGENTS.md instructions. Implement, integrate, and verify the whole ticket. Preserve existing work when resuming or correcting a failed report. Return the required JSON report. Report blockers honestly. The application alone manages Git commits, integration, and publication.".into(), timeout: runner.remaining()?, progress_tx: runner.progress.clone(), cancel: runner.cancel.clone() };
            let outcome = match harness.execute(&request) {
                Ok(outcome) => outcome,
                Err(error) => {
                    runner.remaining()?;
                    let detail = error.detail();
                    fs::write(dir.join(format!("{stamp}-harness-error.txt")), &detail)
                        .map_err(|write_error| anyhow::anyhow!("Harness failed: {detail}\nCould not save diagnostics at {}: {write_error}. Check available disk space and permissions before resuming.", dir.display()))?;
                    if let Ok(report) = fs::read_to_string(&report_path) {
                        crate::harness::HarnessOutcome {
                            final_text: report,
                            envelope: None,
                            stderr_tail: detail,
                        }
                    } else {
                        harness_failures += 1;
                        feedback
                            .push_str(&format!("\nHarness failure {harness_failures}: {detail}\n"));
                        state.detail = feedback.clone();
                        save(dir, state)?;
                        if harness_failures >= 3 {
                            feedback.push_str("\nSELF-REPAIR REQUIRED: repeated harness failures. Diagnose their cause using the saved diagnostics, repair preventable causes in this worktree, and add a regression check. Do not repeat the same failed approach. Write the recovery report file before responding.\n");
                        }
                        anyhow::ensure!(
                            harness_failures <= 5,
                            "Harness recovery exhausted after {harness_failures} failures in this run. Latest failure: {detail}. Full diagnostics are preserved in {}",
                            dir.display()
                        );
                        runner.update(format!("Recovering harness failure {harness_failures}; existing work preserved…"));
                        continue;
                    }
                }
            };
            runner.remaining()?;
            fs::write(
                dir.join(format!("{stamp}-response.txt")),
                &outcome.final_text,
            )?;
            runner.remaining()?;
            let final_is_report =
                crate::harness::pi_extract::extract_json_object(&outcome.final_text)
                    .is_some_and(|json| serde_json::from_str::<Report>(&json).is_ok());
            let report_text = if final_is_report {
                outcome.final_text.clone()
            } else {
                fs::read_to_string(&report_path)
                    .ok()
                    .filter(|text| !text.trim().is_empty())
                    .unwrap_or_else(|| outcome.final_text.clone())
            };
            let parsed = crate::harness::pi_extract::extract_json_object(&report_text)
                .ok_or_else(|| anyhow::anyhow!("No complete JSON implementation report. Return JSON with status, summary, acceptance_criteria, verification, and remaining."))
                .and_then(|json| serde_json::from_str::<Report>(&json).map_err(Into::into));
            // A blocked report is a recovery checkpoint, not an immediate
            // terminal state. Feed its evidence and remaining work through the
            // same bounded correction loop used for report and verification
            // failures. The shared deadline and healing limit still prevent an
            // unrecoverable external dependency from looping forever.
            let blocked_report = parsed
                .as_ref()
                .is_ok_and(|report| report.status == "blocked");
            let mut failure = None;
            let mut verification_failure = false;
            match parsed {
                Err(error) => failure = Some(error.to_string()),
                Ok(report) => {
                    if let Err(error) = validate_report(&report, &state.ticket_text) {
                        failure = Some(error.to_string());
                    } else {
                        state.status = "Verifying".into();
                        save(dir, state)?;
                        let mut evidence = Vec::new();
                        for command in &report.verification {
                            runner.update(format!("Verifying: {command}"));
                            let result = runner.verify(&state.worktree, command);
                            evidence.push(serde_json::json!({"command":command,"output":result.as_ref().ok(),"error":result.as_ref().err().map(ToString::to_string)}));
                            fs::write(
                                dir.join(format!("{stamp}-verification.json")),
                                serde_json::to_vec_pretty(&evidence)?,
                            )?;
                            if let Err(error) = result {
                                verification_failure = true;
                                failure = Some(format!(
                                    "Verification command failed: {command}\n{error}"
                                ));
                                break;
                            }
                        }
                        anyhow::ensure!(
                            runner.git(&state.worktree, &["symbolic-ref", "--short", "HEAD"])?
                                == state.branch,
                            "Implementation changed branches; refusing to publish"
                        );
                        if failure.is_none() {
                            if let Err(error) = runner.git(&state.worktree, &["diff", "--check"]) {
                                verification_failure = true;
                                failure = Some(format!("git diff --check failed: {error}"));
                            }
                        }
                        if failure.is_none() {
                            break report;
                        }
                    }
                }
            }
            let failure = failure.expect("unsuccessful attempt must have a failure");
            let phase = if verification_failure {
                "verification"
            } else {
                "report"
            };
            feedback.push_str(&format!("\nAttempt {attempt} ({phase}): {failure}\n"));
            if blocked_report {
                feedback.push_str("AUTOMATIC BLOCKER RECOVERY REQUIRED: treat the blocked report as a checkpoint, preserve its evidence and completed work, and execute every remaining remediation available from this worktree. Diagnose and repair local tooling, scripts, tests, or implementation defects before reporting blocked again. Do not weaken acceptance criteria or fabricate evidence.\n");
                runner.update(format!(
                    "Recovering reported blocker in the preserved worktree (attempt {attempt})…"
                ));
            }
            state.detail = feedback.clone();
            save(dir, state)?;
            fs::write(dir.join(format!("{stamp}-correction.txt")), &failure)?;
            runner.remaining()?;
            let corrections = if verification_failure {
                &mut verification_corrections
            } else {
                &mut report_corrections
            };
            *corrections += 1;
            if *corrections > 3 {
                anyhow::ensure!(
                    healing_attempts < 2,
                    "Automatic correction limit and self-repair attempts exhausted for {phase} in this run ({attempt} attempts). Latest failure: {failure}\nFull correction evidence is preserved in {}. Resume implementation starts a fresh attempt budget.",
                    dir.display()
                );
                healing_attempts += 1;
                feedback.push_str("\nSELF-REPAIR REQUIRED: ordinary retries are exhausted. Diagnose and fix the root cause in this worktree, add a regression check that reproduces the failure, and rerun the complete verification. Preserve existing task work and checks. Packet will commit the verified repair atomically with this task.\n");
                runner.update(format!(
                    "Diagnosing root cause and self-repairing ({healing_attempts}/2)…"
                ));
            }
            previous_response.clear();
            append_tail(&mut previous_response, &outcome.final_text);
            runner.update(format!(
                "Automatically correcting attempt {attempt}: {feedback}"
            ));
        };
        anyhow::ensure!(
            runner.git(&state.worktree, &["rev-parse", "HEAD"])? == head,
            "Agent changed commit history; refusing a non-atomic task commit"
        );
        let pending_paths = runner.git(&state.worktree, &["diff", "--name-only"])?;
        if !pending_paths.trim().is_empty() {
            let adr = crate::artifacts::packet::publish_adr(
                &state.worktree,
                state,
                &report,
                &pending_paths,
            )?;
            runner.update(format!("Prepared ADR {}", adr.display()));
        }
        runner.git(&state.worktree, &["add", "--all"])?;
        if !runner
            .git(&state.worktree, &["diff", "--cached", "--name-only"])?
            .is_empty()
        {
            runner.git(
                &state.worktree,
                &[
                    "commit",
                    "-m",
                    &format!("Implement {}", title(&state.ticket_text)),
                ],
            )?;
        }
        anyhow::ensure!(
            runner
                .git(&state.worktree, &["status", "--porcelain"])?
                .is_empty(),
            "Worktree is not clean after verification and commit; resume to review"
        );
        let changed_paths = runner.git(
            &state.worktree,
            &[
                "diff",
                "--name-only",
                &format!("{}...HEAD", state.base_commit),
            ],
        )?;
        if changed_paths.is_empty() {
            anyhow::ensure!(
                permits_evidence_only_completion(&state.ticket_text),
                "No implementation changes relative to the starting commit; no PR created"
            );
        }
        state.verified_head = Some(runner.git(&state.worktree, &["rev-parse", "HEAD"])?);
        state.detail = report.summary.clone();
        fs::write(
            dir.join("verified-report.json"),
            serde_json::to_vec_pretty(&report)?,
        )?;
        let body = pr_body(state, &report);
        fs::write(dir.join("pr-body.md"), body)?;
        if changed_paths.is_empty() {
            // Evidence-only tickets deliberately leave the product repository
            // untouched. The verified base commit is their immutable
            // completion anchor, allowing queue dependencies to advance
            // without inventing an empty commit or pull request.
            state.merged_commit = state.verified_head.clone();
            state.status = "Done".into();
            save(dir, state)?;
            return Ok(());
        }
        state.status = "Ready for PR".into();
        save(dir, state)?;
    }
    Ok(())
}

fn execute(
    _planning_root: &Path,
    repo: &Path,
    dir: &Path,
    state: &mut Implementation,
    harness: &dyn AiHarness,
    runner: &Runner,
) -> anyhow::Result<()> {
    prepare_verified(repo, dir, state, harness, runner)?;
    if state.status == "Done" {
        return Ok(());
    }
    if state.auto_merge {
        return auto_publish(repo, dir, state, harness, runner);
    }
    runner.remaining()?;
    runner.update("Publishing the verified implementation and creating its pull request…");
    // Explicit base/head and body file avoid prompts, accidental forks and shell expansion.
    let remote = runner.git(&state.worktree, &["remote", "get-url", "origin"])?;
    let repository = remote_repository(&remote);
    let prs = runner.command(
        &state.worktree,
        &runner.gh,
        &[
            "pr",
            "list",
            "--repo",
            &repository,
            "--head",
            &state.branch,
            "--base",
            &state.base,
            "--state",
            "all",
            "--json",
            "url,state",
        ],
    )?;
    let existing: Vec<serde_json::Value> = serde_json::from_str(&prs)?;
    if let Some(pr) = existing.first() {
        anyhow::ensure!(
            pr["state"] == "OPEN",
            "The existing PR is closed or merged. Review it before publishing more changes; no duplicate PR created."
        );
    }
    runner.git(
        &state.worktree,
        &["push", "--set-upstream", "origin", &state.branch],
    )?;
    if let Some(pr) = existing.first() {
        anyhow::ensure!(
            pr["state"] != "CLOSED",
            "The existing PR is closed. Review it before continuing; no duplicate PR created."
        );
        state.pr_url = pr["url"].as_str().map(String::from);
    } else {
        let body = dir.join("pr-body.md");
        let url = runner.command(
            &state.worktree,
            &runner.gh,
            &[
                "pr",
                "create",
                "--repo",
                &repository,
                "--head",
                &state.branch,
                "--base",
                &state.base,
                "--title",
                &title(&state.ticket_text),
                "--body-file",
                body.to_str()
                    .ok_or_else(|| anyhow::anyhow!("Invalid PR body path"))?,
            ],
        )?;
        state.pr_url = Some(url);
    }
    anyhow::ensure!(
        state
            .pr_url
            .as_ref()
            .is_some_and(|url| url.starts_with("https://")),
        "GitHub did not return a PR URL"
    );
    state.status = "PR created".into();
    state.pr_state = Some("OPEN".into());
    Ok(())
}
fn default_branch(repo: &Path, runner: &Runner) -> anyhow::Result<String> {
    let refs = runner.git(repo, &["ls-remote", "--symref", "origin", "HEAD"])?;
    if let Some(branch) = refs.lines().find_map(|line| {
        line.strip_prefix("ref: refs/heads/")
            .and_then(|rest| rest.split_whitespace().next())
    }) {
        return Ok(branch.into());
    }
    let branch = runner.git(repo, &["symbolic-ref", "--short", "HEAD"])?;
    anyhow::ensure!(
        matches!(branch.as_str(), "main" | "master"),
        "Origin has no default branch; configure its HEAD before Auto mode"
    );
    Ok(branch)
}

fn auto_publish(
    repo: &Path,
    dir: &Path,
    state: &mut Implementation,
    harness: &dyn AiHarness,
    runner: &Runner,
) -> anyhow::Result<()> {
    let publish_lock = fs::OpenOptions::new()
        .read(true)
        .write(true)
        .create(true)
        .truncate(false)
        .open(common(repo)?.join("packet-auto-publish.lock"))?;
    state.status = "Waiting to merge".into();
    save(dir, state)?;
    runner.update("Verified; waiting for the project integration lock…");
    while publish_lock.try_lock().is_err() {
        runner.remaining()?;
        std::thread::sleep(Duration::from_millis(100));
    }
    state.base = default_branch(repo, runner)?;
    let mut last_error = String::new();
    for _ in 0..3 {
        runner.remaining()?;
        runner.update(format!(
            "Integrating and verifying against latest origin/{}…",
            state.base
        ));
        let remote_ref = format!("refs/packet-auto-bases/{}", key(&state.ticket));
        if let Err(error) = runner.git(
            repo,
            &[
                "fetch",
                "--no-tags",
                "--no-write-fetch-head",
                "origin",
                &format!("+refs/heads/{}:{remote_ref}", state.base),
            ],
        ) {
            last_error = error.to_string();
            continue;
        }
        let remote = runner.git(repo, &["rev-parse", &remote_ref])?;
        // Recover a crash or lost push response without another merge or agent call.
        if let Some(commit) = &state.merged_commit {
            if runner
                .git(repo, &["merge-base", "--is-ancestor", commit, &remote])
                .is_ok()
            {
                return finish_auto_publish(repo, dir, state, runner);
            }
        }
        let local = runner.git(repo, &["rev-parse", "HEAD"])?;
        // Integrate on fetched remote truth when histories diverge. The task's
        // verified branch is squash-merged below, with conflicts repaired and
        // verification rerun in isolation. Never rewrite the user's checkout.
        let integration_base = if runner
            .git(repo, &["merge-base", "--is-ancestor", &remote, &local])
            .is_ok()
        {
            local
        } else {
            remote.clone()
        };
        let integration_dir = dir.join(format!("integration-{integration_base}"));
        fs::create_dir_all(&integration_dir)?;
        let mut integration: Implementation = if integration_dir.join("state.json").exists() {
            serde_json::from_slice(&fs::read(integration_dir.join("state.json"))?)?
        } else {
            let mut record = state.clone();
            record.branch = format!(
                "packet/integration/{}/{}",
                key(&state.ticket),
                &integration_base[..12]
            );
            record.worktree = state.worktree.with_file_name(format!(
                "{}-integration-{}",
                key(&state.ticket),
                &integration_base[..12]
            ));
            record.base_commit = integration_base.clone();
            record.verified_head = None;
            record.merged_commit = None;
            record.auto_merge = false;
            record.pr_url = None;
            record.pr_state = None;
            record.status = "Preparing".into();
            record.detail = "Integrate the verified task with the latest default branch. Resolve any merge conflicts preserving both intended behaviors. Repair failures and verify the integrated result.".into();
            save(&integration_dir, &record)?;
            record
        };
        if !integration.worktree.exists() {
            let path = integration
                .worktree
                .to_str()
                .ok_or_else(|| anyhow::anyhow!("Invalid integration path"))?;
            if runner
                .git(
                    repo,
                    &[
                        "show-ref",
                        "--verify",
                        &format!("refs/heads/{}", integration.branch),
                    ],
                )
                .is_ok()
            {
                runner.git(repo, &["worktree", "add", path, &integration.branch])?;
            } else {
                runner.git(
                    repo,
                    &[
                        "worktree",
                        "add",
                        "-b",
                        &integration.branch,
                        path,
                        &integration_base,
                    ],
                )?;
            }
        }
        anyhow::ensure!(
            common(&integration.worktree)?.canonicalize()? == common(repo)?.canonicalize()?
                && runner.git(&integration.worktree, &["symbolic-ref", "--short", "HEAD"])?
                    == integration.branch,
            "Integration worktree identity changed; refusing to modify it"
        );
        if integration.verified_head.is_none() {
            let task_head = state
                .verified_head
                .as_ref()
                .ok_or_else(|| anyhow::anyhow!("Task is not verified"))?;
            let unmerged = runner.git(
                &integration.worktree,
                &["diff", "--name-only", "--diff-filter=U"],
            )?;
            if unmerged.is_empty()
                && runner
                    .git(&integration.worktree, &["status", "--porcelain"])?
                    .is_empty()
            {
                if let Err(error) =
                    runner.git(&integration.worktree, &["merge", "--squash", task_head])
                {
                    anyhow::ensure!(
                        !runner
                            .git(
                                &integration.worktree,
                                &["diff", "--name-only", "--diff-filter=U"]
                            )?
                            .is_empty(),
                        "Cannot integrate task: {error}"
                    );
                    integration
                        .detail
                        .push_str(&format!("\nMerge failed: {error}"));
                }
            }
            let report: Report =
                serde_json::from_slice(&fs::read(dir.join("verified-report.json"))?)?;
            let mut check_error = None;
            let mut evidence = Vec::new();
            if runner
                .git(
                    &integration.worktree,
                    &["diff", "--name-only", "--diff-filter=U"],
                )?
                .is_empty()
            {
                for command in &report.verification {
                    runner.update(format!("Verifying integrated task: {command}"));
                    let result = runner.verify(&integration.worktree, command);
                    evidence.push(serde_json::json!({"command":command,"output":result.as_ref().ok(),"error":result.as_ref().err().map(ToString::to_string)}));
                    if let Err(error) = result {
                        check_error = Some(error.to_string());
                        break;
                    }
                }
                if check_error.is_none() {
                    check_error = runner
                        .git(&integration.worktree, &["diff", "--check"])
                        .err()
                        .map(|e| e.to_string());
                }
            } else {
                check_error =
                    Some("Resolve the pending squash-merge conflicts before verification".into());
            }
            fs::write(
                integration_dir.join(format!(
                    "{}-verification.json",
                    chrono::Utc::now().timestamp_nanos_opt().unwrap_or_default()
                )),
                serde_json::to_vec_pretty(&evidence)?,
            )?;
            if let Some(error) = check_error {
                integration
                    .detail
                    .push_str(&format!("\nIntegration verification failure: {error}"));
                save(&integration_dir, &integration)?;
                prepare_verified(repo, &integration_dir, &mut integration, harness, runner)?;
            } else {
                runner.git(&integration.worktree, &["add", "--all"])?;
                if !runner
                    .git(&integration.worktree, &["diff", "--cached", "--name-only"])?
                    .is_empty()
                {
                    runner.git(
                        &integration.worktree,
                        &[
                            "commit",
                            "-m",
                            &format!("Implement {}", title(&state.ticket_text)),
                        ],
                    )?;
                }
                integration.verified_head =
                    Some(runner.git(&integration.worktree, &["rev-parse", "HEAD"])?);
                save(&integration_dir, &integration)?;
            }
        }
        anyhow::ensure!(
            runner
                .git(&integration.worktree, &["status", "--porcelain"])?
                .is_empty()
                && integration.verified_head.as_deref()
                    == Some(
                        runner
                            .git(&integration.worktree, &["rev-parse", "HEAD"])?
                            .as_str()
                    ),
            "Integrated result changed after verification"
        );
        state.merged_commit = integration.verified_head.clone();
        state.status = "Publishing".into();
        save(dir, state)?;
        let target = format!(
            "{}:refs/heads/{}",
            state.merged_commit.as_ref().unwrap(),
            state.base
        );
        match runner.git(&integration.worktree, &["push", "origin", &target]) {
            Ok(_) => {
                return finish_auto_publish(repo, dir, state, runner);
            }
            Err(error) => {
                last_error = error.to_string();
                runner.update(
                    "Default branch changed or push failed; fetching and retrying integration…",
                );
            }
        }
    }
    anyhow::bail!(
        "Auto publication could not complete after retries; verified work is preserved: {last_error}"
    )
}

fn finish_auto_publish(
    repo: &Path,
    dir: &Path,
    state: &mut Implementation,
    runner: &Runner,
) -> anyhow::Result<()> {
    state.status = "Done".into();
    save(dir, state)?;
    // Remote publication is already durable. Update an idle clean checkout only;
    // a dirty or divergent checkout remains untouched and does not undo success.
    // The writer gate keeps this fast-forward from trampling an in-flight
    // planning checkpoint (turns may commit while workers publish).
    let _guard = crate::core::writer_gate::acquire();
    if runner
        .git(repo, &["status", "--porcelain"])
        .is_ok_and(|status| status.is_empty())
        && runner
            .git(repo, &["symbolic-ref", "--short", "HEAD"])
            .is_ok_and(|branch| branch == state.base)
    {
        if let Some(commit) = &state.merged_commit {
            if let Err(error) = runner.git(repo, &["merge", "--ff-only", commit]) {
                runner.update(format!(
                    "Task merged remotely; local checkout could not fast-forward: {error}"
                ));
            }
        }
    }
    Ok(())
}

fn remote_repository(remote: &str) -> String {
    let remote = remote.trim().trim_end_matches('/').trim_end_matches(".git");
    if let Some(path) = remote.strip_prefix("git@") {
        return path.replacen(':', "/", 1);
    }
    for scheme in ["https://", "http://", "ssh://"] {
        if let Some(path) = remote.strip_prefix(scheme) {
            return path
                .rsplit_once('@')
                .map(|(_, p)| p)
                .unwrap_or(path)
                .to_owned();
        }
    }
    remote.to_owned()
}
fn title(ticket: &str) -> String {
    ticket
        .lines()
        .next()
        .unwrap_or("Implement ticket")
        .trim_start_matches('#')
        .trim()
        .chars()
        .take(200)
        .collect()
}
fn validate_report(report: &Report, ticket: &str) -> anyhow::Result<()> {
    anyhow::ensure!(
        report.status == "complete" && report.remaining.is_empty(),
        "## Implementation blocked\n\n### What is complete\n\n{}\n\n### Next action(s)\n\n- {}",
        report.summary,
        if report.remaining.is_empty() {
            "Review the report and choose Resume implementation.".to_owned()
        } else {
            report.remaining.join("\n- ")
        }
    );
    anyhow::ensure!(
        !report.summary.trim().is_empty()
            && !report.acceptance_criteria.is_empty()
            && report
                .acceptance_criteria
                .iter()
                .all(|c| !c.criterion.trim().is_empty() && !c.evidence.trim().is_empty()),
        "Report lacks acceptance-criterion evidence"
    );
    let mut in_criteria = false;
    for line in ticket.lines() {
        if line.starts_with("## ") {
            in_criteria = line.trim().eq_ignore_ascii_case("## Acceptance criteria");
            continue;
        }
        if in_criteria {
            if let Some(criterion) = line.trim().strip_prefix("- ") {
                anyhow::ensure!(
                    report
                        .acceptance_criteria
                        .iter()
                        .any(|c| c.criterion.trim() == criterion.trim()),
                    "Missing evidence for ticket acceptance criterion: {criterion}"
                );
            }
        }
    }
    anyhow::ensure!(
        !report.verification.is_empty()
            && report
                .verification
                .iter()
                .all(|c| !c.trim().is_empty() && !matches!(c.trim(), "true" | ":" | "exit 0")),
        "Report lacks meaningful verification commands"
    );
    Ok(())
}

/// Recognize the deliberately narrow contract used by audit/demonstration
/// tickets. Requiring all three independent statements prevents an ordinary
/// implementation task from turning a no-op report into a successful result.
pub(crate) fn permits_evidence_only_completion(ticket: &str) -> bool {
    let contract = ticket.to_ascii_lowercase();
    [
        "this ticket lands zero planner code",
        "explicitly unchanged",
        "no product-repo file may be created or modified by this ticket",
    ]
    .iter()
    .all(|statement| contract.contains(statement))
        || ([
            "pure verification",
            "commits no bytes",
            "this ticket itself changed no repository file",
        ]
        .iter()
        .all(|statement| contract.contains(statement)))
}
fn pr_body(state: &Implementation, report: &Report) -> String {
    let mut text = format!(
        "{}\n\nTicket: `{}`\n\n## Acceptance criteria\n\n",
        report.summary, state.ticket
    );
    for c in &report.acceptance_criteria {
        text.push_str(&format!("- {}: {}\n", c.criterion, c.evidence));
    }
    text.push_str("\n## Validation\n\nPacket reran these commands successfully in the implementation worktree:\n\n");
    for c in &report.verification {
        text.push_str(&format!("```sh\n{c}\n```\n\n"));
    }
    text
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::error::AppError;
    use std::sync::atomic::AtomicUsize;
    #[test]
    fn disconnected_worker_reports_failure_instead_of_waiting_forever() {
        let mut controller = Controller::idle_fixture();
        assert!(controller.poll().is_none());
        controller._keep_alive.take();
        assert!(matches!(controller.poll(), Some(Event::Done(Err(message))) if message.contains("stopped without a result")));
    }

    #[test]
    fn completed_worker_delivers_result_before_disconnect() {
        let controller = Controller::idle_fixture();
        controller._keep_alive.as_ref().unwrap().send(Event::Done(Err("original cause".into()))).unwrap();
        assert!(matches!(controller.poll(), Some(Event::Done(Err(message))) if message == "original cause"));
    }

    #[test]
    fn implementation_context_uses_only_frozen_affected_product_modules() {
        let root = std::env::temp_dir().join(format!(
            "packet_implementation_context_{}-{}",
            std::process::id(),
            chrono::Utc::now().timestamp_nanos_opt().unwrap()
        ));
        let batch = root.join("planning/tasks/feature");
        fs::create_dir_all(&batch).unwrap();
        let contract = crate::core::contract_snapshot::BatchContract {
            feature_id: "CHG-001".into(),
            feature_specification: "# Feature".into(),
            product_modules: [(
                "05-functional-requirements".into(),
                "## 5. Relevant\n".into(),
            )]
            .into(),
            repository_bases: Default::default(),
            configuration: String::new(),
        };
        fs::write(
            batch.join("contract.json"),
            serde_json::to_vec(&contract).unwrap(),
        )
        .unwrap();
        let context = scoped_product_context(&root, "planning/tasks/feature/001-task.md")
            .unwrap()
            .unwrap();
        assert!(context.contains("product:05-functional-requirements"));
        assert!(context.contains("## 5. Relevant"));
        assert!(!context.contains("product:01-vision"));
        let _ = fs::remove_dir_all(root);
    }
    #[test]
    fn cross_repository_dependency_context_requires_merged_record() {
        let root = std::env::temp_dir().join(format!(
            "packet_dependency_context_{}-{}",
            std::process::id(),
            chrono::Utc::now().timestamp_nanos_opt().unwrap()
        ));
        let batch = root.join("planning/tasks/feature");
        fs::create_dir_all(&batch).unwrap();
        assert!(
            std::process::Command::new("git")
                .args(["init", "-q"])
                .current_dir(&root)
                .status()
                .unwrap()
                .success()
        );
        let prior = "planning/tasks/feature/001-api.md";
        let prior_text =
            "# API contract\n\nRepository: api\n\nThe endpoint returns a saved search.\n";
        fs::write(root.join(prior), prior_text).unwrap();
        let next = "planning/tasks/feature/002-web.md";
        let next_text =
            "# Web client\n\nRepository: web\n\n## Dependencies\n\n- [API contract](001-api.md)\n";
        assert!(completed_dependency_context(&root, next, next_text).is_err());
        let record: Implementation = serde_json::from_value(serde_json::json!({
            "ticket": prior, "ticket_text": prior_text, "branch": "packet/api", "base": "main",
            "base_commit": "base", "worktree": root, "status": "Done", "detail": "",
            "pr_url": null, "verified_head": "merged", "merged_commit": "merged"
        }))
        .unwrap();
        let dir = state_dir(&root, prior).unwrap();
        fs::create_dir_all(&dir).unwrap();
        fs::write(dir.join("state.json"), serde_json::to_vec(&record).unwrap()).unwrap();
        let context = completed_dependency_context(&root, next, next_text)
            .unwrap()
            .unwrap();
        assert!(context.contains("Repository: api"));
        assert!(context.contains("Merged commit: merged"));
        assert!(!context.contains("Repository: web"));
        let _ = fs::remove_dir_all(root);
    }
    struct Fixture {
        mode: &'static str,
        calls: Arc<AtomicUsize>,
    }
    impl AiHarness for Fixture {
        fn label(&self) -> String {
            "implementation fixture".into()
        }
        fn check_available(&self) -> Result<String, AppError> {
            Ok("fixture".into())
        }
        fn execute(
            &self,
            req: &PlanningRequest,
        ) -> Result<crate::harness::HarnessOutcome, AppError> {
            let call = self.calls.fetch_add(1, Ordering::SeqCst);
            assert!(req.implementation);
            assert!(req.prompt_body.contains("RESUME"));
            if self.mode == "fresh_budget" {
                assert!(req.prompt_body.contains("fresh report, verification, harness, and self-repair budget"));
                assert!(!req.prompt_body.contains("Automatic correction limit and self-repair attempts exhausted"));
                assert!(!req.prompt_body.contains("Correction history: Correction history:"));
            }
            if self.mode.starts_with("repair_") && call > 0 {
                assert!(
                    req.prompt_body
                        .contains("PREVIOUS STOP / CORRECTION REQUIRED")
                );
                assert!(req.prompt_body.contains("Previous response"));
                assert_eq!(
                    fs::read_to_string(req.repo_root.join("implemented.txt")).unwrap(),
                    "implemented\n"
                );
                if self.mode == "repair_verification" || (self.mode == "repair_mixed" && call > 3) {
                    assert!(
                        req.prompt_body
                            .contains("Verification command failed: test -f missing-file")
                    );
                    fs::write(
                        req.repo_root.join("missing-file"),
                        "repaired prerequisite\n",
                    )
                    .unwrap();
                }
                if self.mode == "repair_blocked" {
                    assert!(
                        req.prompt_body
                            .contains("AUTOMATIC BLOCKER RECOVERY REQUIRED")
                    );
                    assert!(req.prompt_body.contains("Repair the local conductor"));
                }
                if self.mode == "repair_cancel" {
                    req.cancel.store(true, Ordering::SeqCst);
                }
            }
            if self.mode == "resume" {
                assert!(
                    req.prompt_body
                        .contains("PREVIOUS STOP / CORRECTION REQUIRED")
                );
                assert!(req.prompt_body.contains("Implementation cancelled"));
                assert_eq!(
                    fs::read_to_string(req.repo_root.join("implemented.txt")).unwrap(),
                    "partial\n"
                );
            }
            if self.mode == "cancel" {
                fs::write(req.repo_root.join("implemented.txt"), "partial\n").unwrap();
                req.cancel.store(true, Ordering::SeqCst);
            } else if self.mode != "evidence_only" {
                fs::write(req.repo_root.join("implemented.txt"), "implemented\n").unwrap();
            }
            if self.mode == "harness_retry" && call == 0 || self.mode == "harness_dead" {
                return Err(AppError::HarnessFailed {
                    reason: "pi finished but produced no final assistant message".into(),
                    stderr_tail: String::new(),
                });
            }
            if self.mode == "healing" && call >= 4 {
                assert!(req.prompt_body.contains("SELF-REPAIR REQUIRED"));
            }
            let status = if self.mode == "blocked" || (self.mode == "repair_blocked" && call == 0) {
                "blocked"
            } else {
                "complete"
            };
            let verification = if self.mode == "evidence_only" {
                "test ! -e implemented.txt"
            } else if self.mode == "fail"
                || self.mode == "repair_verification"
                || self.mode == "repair_mixed"
            {
                "test -f missing-file"
            } else {
                "test \"$(cat implemented.txt)\" = implemented"
            };
            let criterion = if self.mode == "evidence_only" {
                "Repository remains unchanged."
            } else {
                "File contains implemented."
            };
            let mut report = serde_json::json!({"status":status,"summary":"Implemented the ticket behavior.","acceptance_criteria":[{"criterion":criterion,"evidence":"Created the required evidence and checked its exact contents."}],"verification":[verification],"remaining":[]});
            if status == "blocked" {
                report["remaining"] = serde_json::json!([
                    "Repair the local conductor and rerun the acceptance check."
                ]);
            }
            if call == 0 && self.mode == "repair_criterion" {
                report["acceptance_criteria"][0]["criterion"] =
                    "File contains implementation.".into();
            }
            let final_text = if (self.mode == "healing" && call < 4)
                || (self.mode == "repair_mixed" && call < 3)
                || (call == 0 && matches!(self.mode, "repair_markdown" | "repair_cancel"))
            {
                "# Summary\nImplemented the ticket.".into()
            } else if call == 0 && self.mode == "repair_schema" {
                "{\"status\":\"complete\"}".into()
            } else {
                report.to_string()
            };
            if self.mode == "sidecar" {
                let path = req
                    .prompt_body
                    .lines()
                    .find_map(|line| line.strip_prefix("RECOVERY REPORT FILE: "))
                    .unwrap();
                fs::write(path, &final_text).unwrap();
                return Err(AppError::HarnessFailed {
                    reason: "pi finished but produced no final assistant message".into(),
                    stderr_tail: String::new(),
                });
            }
            Ok(crate::harness::HarnessOutcome {
                final_text,
                envelope: None,
                stderr_tail: String::new(),
            })
        }
    }
    struct Sandbox {
        root: PathBuf,
        repo: PathBuf,
        gh: PathBuf,
        ticket: String,
    }
    impl Sandbox {
        fn git(&self, cwd: &Path, args: &[&str]) -> String {
            let output = std::process::Command::new("git")
                .args(args)
                .current_dir(cwd)
                .output()
                .unwrap();
            assert!(
                output.status.success(),
                "git {} failed: {}",
                args.join(" "),
                String::from_utf8_lossy(&output.stderr)
            );
            String::from_utf8(output.stdout).unwrap().trim().into()
        }
        fn advance_remote(&self) -> String {
            let peer = self.root.join("peer");
            self.git(
                &self.root,
                &[
                    "clone",
                    "-q",
                    "-b",
                    "main",
                    self.root.join("remote.git").to_str().unwrap(),
                    peer.to_str().unwrap(),
                ],
            );
            self.git(&peer, &["config", "user.name", "Fixture"]);
            self.git(&peer, &["config", "user.email", "fixture@example.test"]);
            fs::write(peer.join("upstream.txt"), "latest upstream\n").unwrap();
            self.git(&peer, &["add", "."]);
            self.git(&peer, &["commit", "-qm", "upstream update"]);
            self.git(&peer, &["push", "-q", "origin", "main"]);
            self.git(&peer, &["rev-parse", "HEAD"])
        }
        fn new() -> Self {
            let root = std::env::temp_dir().join(format!(
                "packet-implementation-{}-{}",
                std::process::id(),
                chrono::Utc::now().timestamp_nanos_opt().unwrap()
            ));
            fs::create_dir_all(&root).unwrap();
            let repo = root.join("repo");
            fs::create_dir(&repo).unwrap();
            let git = |args: &[&str]| {
                let o = std::process::Command::new("git")
                    .args(args)
                    .current_dir(&repo)
                    .output()
                    .unwrap();
                assert!(
                    o.status.success(),
                    "git {} failed: {}",
                    args.join(" "),
                    String::from_utf8_lossy(&o.stderr)
                );
            };
            git(&["init", "-q", "-b", "main"]);
            git(&["config", "user.name", "Fixture"]);
            git(&["config", "user.email", "fixture@example.test"]);
            let ticket = "planning/tasks/feature/001-implement-ticket-behavior.md".to_owned();
            fs::create_dir_all(repo.join("planning/tasks/feature")).unwrap();
            fs::write(repo.join(&ticket), "# Implement ticket behavior\n\n## Acceptance criteria\n\n- File contains implemented.\n").unwrap();
            git(&["add", "."]);
            git(&["commit", "-qm", "baseline"]);
            git(&[
                "init",
                "--bare",
                "-q",
                root.join("remote.git").to_str().unwrap(),
            ]);
            git(&[
                "remote",
                "add",
                "origin",
                root.join("remote.git").to_str().unwrap(),
            ]);
            git(&["push", "-q", "origin", "main"]);
            let gh = root.join("gh-fixture");
            fs::write(&gh, "#!/bin/sh\nroot=$(dirname \"$0\")\nif [ -f \"$root/offline\" ]; then echo 'simulated GitHub unavailable' >&2; exit 1; fi\nif [ \"$2\" = list ]; then\n if [ -f \"$root/pr-created\" ]; then echo '[{\"url\":\"https://github.com/fixture/repo/pull/1\",\"state\":\"OPEN\"}]'; else echo '[]'; fi\nelse\n echo created >> \"$root/pr-created\"\n echo 'https://github.com/fixture/repo/pull/1'\nfi\n").unwrap();
            #[cfg(unix)]
            {
                use std::os::unix::fs::PermissionsExt;
                fs::set_permissions(&gh, fs::Permissions::from_mode(0o700)).unwrap();
            }
            Self {
                root,
                repo,
                gh,
                ticket,
            }
        }
        fn run(
            &self,
            mode: &'static str,
            calls: Arc<AtomicUsize>,
        ) -> anyhow::Result<Implementation> {
            let (tx, _rx) = mpsc::channel();
            run_with_gh(
                &self.repo,
                &self.ticket,
                &Fixture { mode, calls },
                Arc::new(AtomicBool::new(false)),
                tx,
                self.gh.to_str().unwrap(),
            )
        }
        fn make_evidence_only(&self) {
            fs::write(
                self.repo.join(&self.ticket),
                "# Conduct audit\n\nThis ticket lands zero planner code.\n\nAll source files are explicitly unchanged.\n\nNo product-repo file may be created or modified by this ticket.\n\n## Acceptance criteria\n\n- Repository remains unchanged.\n",
            )
            .unwrap();
            self.git(&self.repo, &["add", &self.ticket]);
            self.git(&self.repo, &["commit", "-qm", "define evidence-only audit"]);
            self.git(&self.repo, &["push", "-q", "origin", "main"]);
        }
    }
    impl Drop for Sandbox {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.root);
        }
    }
    #[test]
    fn isolated_implementation_verifies_pushes_and_reuses_pr() {
        let s = Sandbox::new();
        fs::write(s.repo.join("unrelated.txt"), "main checkout draft").unwrap();
        let calls = Arc::new(AtomicUsize::new(0));
        let result = s.run("complete", calls.clone()).unwrap();
        assert_eq!(result.status, "PR created");
        assert!(result.pr_url.is_some());
        assert!(!s.repo.join("implemented.txt").exists());
        assert!(!result.worktree.join("unrelated.txt").exists());
        assert_eq!(
            fs::read_to_string(s.repo.join("unrelated.txt")).unwrap(),
            "main checkout draft"
        );
        // Simulate a crash after GitHub created the PR but before its URL was saved.
        let mut recovery = result.clone();
        recovery.pr_url = None;
        save(&state_dir(&s.repo, &s.ticket).unwrap(), &recovery).unwrap();
        let again = s.run("complete", calls.clone()).unwrap();
        assert_eq!(again.pr_url, result.pr_url);
        assert_eq!(calls.load(Ordering::SeqCst), 1);
        assert_eq!(
            fs::read_to_string(s.root.join("pr-created"))
                .unwrap()
                .lines()
                .count(),
            1
        );
    }
    #[test]
    fn explicit_evidence_only_task_completes_without_commit_or_pr() {
        let s = Sandbox::new();
        s.make_evidence_only();
        let base = s.git(&s.repo, &["rev-parse", "HEAD"]);
        let calls = Arc::new(AtomicUsize::new(0));

        let result = s.run("evidence_only", calls.clone()).unwrap();

        assert_eq!(calls.load(Ordering::SeqCst), 1);
        assert_eq!(result.status, "Done");
        assert_eq!(result.verified_head.as_deref(), Some(base.as_str()));
        assert_eq!(result.merged_commit.as_deref(), Some(base.as_str()));
        assert!(result.pr_url.is_none());
        assert!(!s.root.join("pr-created").exists());
        assert!(result.cleanup.completed_at.is_some(), "{:?}", result.cleanup);
        assert!(!result.worktree.exists());
        assert_eq!(s.git(&s.repo, &["rev-parse", "HEAD"]), base);
        assert!(state_dir(&s.repo, &s.ticket).unwrap().join("verified-report.json").exists());
    }
    #[test]
    fn verification_contract_completes_without_product_changes() {
        let s = Sandbox::new();
        fs::write(s.repo.join(&s.ticket), "# Verify regression\n\nThis ticket is pure verification, commits no bytes.\n\n## Acceptance criteria\n\n- Repository remains unchanged.\n\nThis ticket itself changed no repository file.\n").unwrap();
        let result = s
            .run("evidence_only", Arc::new(AtomicUsize::new(0)))
            .unwrap();
        assert_eq!(result.status, "Done");
        assert!(result.pr_url.is_none());
        assert!(!permits_evidence_only_completion(
            "Implement a feature with pure verification."
        ));
    }

    #[test]
    fn cancelled_worktree_is_reviewed_and_resumed() {
        let s = Sandbox::new();
        let calls = Arc::new(AtomicUsize::new(0));
        assert!(s.run("cancel", calls.clone()).is_err());
        let state = load(&s.repo, &s.ticket).unwrap();
        assert_eq!(state.status, "Interrupted");
        assert!(!s.root.join("pr-created").exists());
        s.advance_remote();
        let resumed = s.run("resume", calls.clone()).unwrap();
        assert_eq!(resumed.worktree, state.worktree);
        assert_eq!(resumed.base_commit, state.base_commit);
        assert!(!resumed.worktree.join("upstream.txt").exists());
        assert_eq!(calls.load(Ordering::SeqCst), 2);
    }

    #[test]
    fn feature_named_workspace_ticket_resumes_preserved_work() {
        let mut s = Sandbox::new();
        let ticket = ".kool-ade-packet/planning/tasks/feature/CHG-003-TASK-verify-workspace.md";
        fs::create_dir_all(s.repo.join(ticket).parent().unwrap()).unwrap();
        fs::rename(s.repo.join(&s.ticket), s.repo.join(ticket)).unwrap();
        s.ticket = ticket.into();
        s.git(&s.repo, &["add", "."]);
        s.git(&s.repo, &["commit", "-qm", "feature-named ticket"]);
        s.git(&s.repo, &["push", "-q", "origin", "main"]);
        let calls = Arc::new(AtomicUsize::new(0));
        assert!(s.run("cancel", calls.clone()).is_err());
        let mut state = load(&s.repo, ticket).unwrap();
        state.status = "Needs attention".into();
        save(&state_dir(&s.repo, ticket).unwrap(), &state).unwrap();
        let resumed = s.run("resume", calls.clone()).unwrap();
        assert_eq!(resumed.worktree, state.worktree);
        assert_eq!(resumed.base_commit, state.base_commit);
        assert_eq!(resumed.status, "PR created");
        assert_eq!(calls.load(Ordering::SeqCst), 2);
    }

    #[test]
    fn implementation_uses_board_filename_rules_in_both_task_roots() {
        let s = Sandbox::new();
        for root in ["planning/tasks", ".kool-ade-packet/planning/tasks"] {
            fs::create_dir_all(s.repo.join(root).join("validation")).unwrap();
            for name in ["001-task.md", "CHG-003-TASK-verify.md", "F10-TASK-verify.md",
                "README.md", "specification.md", "001-task.txt", "invalid-TASK-verify.md"] {
                let path = format!("{root}/validation/{name}");
                fs::write(s.repo.join(&path), "# Fixture task").unwrap();
                assert_eq!(read_ticket(&s.repo, &path).is_ok(),
                    crate::artifacts::task_docs::is_task_story_filename(name), "{path}");
            }
        }
        assert!(read_ticket(&s.repo, "planning/tasks/../001-task.md").is_err());
    }

    #[test]
    fn new_task_includes_latest_remote_without_touching_checkout() {
        let s = Sandbox::new();
        let original = s.git(&s.repo, &["rev-parse", "HEAD"]);
        let latest = s.advance_remote();
        fs::write(s.repo.join("draft.txt"), "keep my draft").unwrap();
        let result = s.run("complete", Arc::new(AtomicUsize::new(0))).unwrap();
        assert_eq!(result.base_commit, latest);
        assert_eq!(
            fs::read_to_string(result.worktree.join("upstream.txt")).unwrap(),
            "latest upstream\n"
        );
        assert_eq!(s.git(&s.repo, &["rev-parse", "HEAD"]), original);
        assert_eq!(
            fs::read_to_string(s.repo.join("draft.txt")).unwrap(),
            "keep my draft"
        );
        assert!(!s.repo.join("upstream.txt").exists());
    }

    #[test]
    fn divergence_and_fetch_failure_stop_before_agent_runs() {
        for diverged in [true, false] {
            let s = Sandbox::new();
            if diverged {
                s.advance_remote();
                fs::write(s.repo.join("local.txt"), "local change").unwrap();
                s.git(&s.repo, &["add", "."]);
                s.git(&s.repo, &["commit", "-qm", "local change"]);
            } else {
                s.git(
                    &s.repo,
                    &["remote", "set-url", "origin", "/nonexistent-packet-remote"],
                );
            }
            let original = s.git(&s.repo, &["rev-parse", "HEAD"]);
            let calls = Arc::new(AtomicUsize::new(0));
            let error = s.run("complete", calls.clone()).unwrap_err().to_string();
            assert!(error.contains(if diverged { "diverged" } else { "failed" }));
            assert_eq!(calls.load(Ordering::SeqCst), 0);
            assert_eq!(s.git(&s.repo, &["rev-parse", "HEAD"]), original);
            assert!(load(&s.repo, &s.ticket).is_none());
        }
    }

    #[test]
    fn local_commits_ahead_of_remote_are_preserved() {
        let s = Sandbox::new();
        fs::write(s.repo.join("local.txt"), "local change").unwrap();
        s.git(&s.repo, &["add", "."]);
        s.git(&s.repo, &["commit", "-qm", "local change"]);
        let original = s.git(&s.repo, &["rev-parse", "HEAD"]);
        let state = s.run("complete", Arc::new(AtomicUsize::new(0))).unwrap();
        assert_eq!(state.base_commit, original);
        assert!(state.worktree.join("local.txt").exists());
    }

    #[test]
    fn pr_checks_persist_closed_reopened_merged_and_keep_state_on_failure() {
        let s = Sandbox::new();
        s.run("complete", Arc::new(AtomicUsize::new(0))).unwrap();
        let (progress, _rx) = mpsc::channel();
        let runner = Runner {
            gh: s.gh.to_string_lossy().into(),
            deadline: Instant::now() + Duration::from_secs(20),
            cancel: Arc::new(AtomicBool::new(false)),
            progress,
        };
        for (value, column) in [("CLOSED", 3), ("OPEN", 2), ("MERGED", 4)] {
            fs::write(
                &s.gh,
                format!("#!/bin/sh\nprintf '%s\\n' '{{\"state\":\"{value}\"}}'\n"),
            )
            .unwrap();
            refresh_pr(&s.repo, &s.ticket, &runner).unwrap();
            let state = load(&s.repo, &s.ticket).unwrap();
            assert_eq!(state.pr_state.as_deref(), Some(value));
            assert_eq!(board_column(Some(&state), false), column);
            assert!(state.pr_checked_at.is_some());
            assert!(state.pr_check_error.is_none());
            if value != "MERGED" {
                for output in ["echo offline >&2; exit 1", "echo '{\"state\":\"UNKNOWN\"}'"] {
                    fs::write(&s.gh, format!("#!/bin/sh\n{output}\n")).unwrap();
                    refresh_pr(&s.repo, &s.ticket, &runner).unwrap();
                    let failed = load(&s.repo, &s.ticket).unwrap();
                    assert_eq!(failed.pr_state, state.pr_state);
                    assert_eq!(failed.pr_checked_at, state.pr_checked_at);
                    assert!(failed.pr_check_error.is_some());
                }
            }
        }
        assert_eq!(load_all(&s.repo).len(), 1);
        assert_eq!(load(&s.repo, &s.ticket).unwrap().status, "Done");
    }

    #[test]
    fn pr_refresh_does_not_overwrite_an_active_implementation() {
        let s = Sandbox::new();
        s.run("complete", Arc::new(AtomicUsize::new(0))).unwrap();
        let dir = state_dir(&s.repo, &s.ticket).unwrap();
        let before = fs::read(dir.join("state.json")).unwrap();
        let lock = fs::OpenOptions::new()
            .read(true)
            .write(true)
            .open(dir.join("run.lock"))
            .unwrap();
        lock.lock().unwrap();
        let (progress, _rx) = mpsc::channel();
        let runner = Runner {
            gh: "must-not-run".into(),
            deadline: Instant::now() + Duration::from_secs(5),
            cancel: Arc::new(AtomicBool::new(false)),
            progress,
        };
        let started = Instant::now();
        refresh_pr(&s.repo, &s.ticket, &runner).unwrap();
        // The deliberate no-op must stay bounded: a held lock may add at most
        // the short settle budget, never an unbounded block.
        assert!(started.elapsed() < Duration::from_secs(2));
        assert_eq!(fs::read(dir.join("state.json")).unwrap(), before);
    }
    #[test]
    fn blocked_or_failed_verification_never_creates_pr() {
        for mode in ["blocked", "fail"] {
            let s = Sandbox::new();
            let outcome = s.run(mode, Arc::new(AtomicUsize::new(0)));
            assert!(
                outcome.is_err(),
                "mode {mode} must fail: {:?}",
                outcome.ok()
            );
            assert!(!s.root.join("pr-created").exists());
            let Some(state) = load(&s.repo, &s.ticket) else {
                panic!(
                    "mode {mode}: run failed ({outcome:?}) before persisting workflow state; \n\
                     a git/io-level infrastructure fault is suspected rather than the \n\
                     verification mode under test"
                );
            };
            assert!(state.worktree.join("implemented.txt").exists());
        }
    }
    #[test]
    fn automatic_corrections_preserve_work_and_publish_only_after_verification() {
        for mode in [
            "repair_markdown",
            "repair_schema",
            "repair_criterion",
            "repair_verification",
            "repair_blocked",
        ] {
            let s = Sandbox::new();
            let calls = Arc::new(AtomicUsize::new(0));
            let result = s.run(mode, calls.clone()).unwrap();
            assert_eq!(calls.load(Ordering::SeqCst), 2, "{mode}");
            assert_eq!(result.status, "PR created");
            let dir = state_dir(&s.repo, &s.ticket).unwrap();
            let files = fs::read_dir(&dir)
                .unwrap()
                .map(|entry| entry.unwrap().file_name().to_string_lossy().into_owned())
                .collect::<Vec<_>>();
            assert_eq!(
                files
                    .iter()
                    .filter(|name| name.ends_with("-response.txt"))
                    .count(),
                2
            );
            assert_eq!(
                files
                    .iter()
                    .filter(|name| name.ends_with("-correction.txt"))
                    .count(),
                1
            );
            if mode == "repair_verification" {
                assert_eq!(
                    files
                        .iter()
                        .filter(|name| name.ends_with("-verification.json"))
                        .count(),
                    2
                );
                assert!(result.worktree.join("missing-file").exists());
            }
            assert_eq!(
                fs::read_to_string(s.root.join("pr-created"))
                    .unwrap()
                    .lines()
                    .count(),
                1
            );
        }
    }

    #[test]
    fn correction_limit_blocker_and_cancellation_never_publish() {
        for (mode, expected_calls, error_text) in [
            ("fail", 6, "Automatic correction limit"),
            ("blocked", 6, "Automatic correction limit"),
            ("repair_cancel", 2, "Implementation cancelled"),
        ] {
            let s = Sandbox::new();
            let calls = Arc::new(AtomicUsize::new(0));
            let error = s.run(mode, calls.clone()).unwrap_err().to_string();
            assert!(error.contains(error_text), "{error}");
            assert_eq!(calls.load(Ordering::SeqCst), expected_calls);
            assert!(!s.root.join("pr-created").exists());
            let state = load(&s.repo, &s.ticket).unwrap();
            assert!(state.worktree.join("implemented.txt").exists());
            assert!(state.detail.contains(error_text));
        }
    }

    #[test]
    fn resume_after_exhaustion_gets_full_budget_without_nested_history() {
        let s = Sandbox::new();
        let calls = Arc::new(AtomicUsize::new(0));
        let first = s.run("fail", calls.clone()).unwrap_err().to_string();
        let original = load(&s.repo, &s.ticket).unwrap();
        assert_eq!(calls.load(Ordering::SeqCst), 6);
        let second = s.run("fail", calls.clone()).unwrap_err().to_string();
        assert_eq!(calls.load(Ordering::SeqCst), 12, "resume gets all six attempts again");
        assert_eq!(second.matches("Automatic correction limit").count(), 1);
        assert!(!second.contains("Correction history:"));
        assert!(second.contains("Latest failure:"));
        assert!(!s.root.join("pr-created").exists());
        let dir = state_dir(&s.repo, &s.ticket).unwrap();
        assert!(fs::read_dir(&dir).unwrap().flatten().any(|entry|
            entry.file_name().to_string_lossy().ends_with("-resume-context.txt")
                && fs::read_to_string(entry.path()).unwrap() == first));
        let resumed = s.run("fresh_budget", Arc::new(AtomicUsize::new(0))).unwrap();
        assert_eq!(resumed.worktree, original.worktree);
        assert_eq!(resumed.base_commit, original.base_commit);
        assert_eq!(resumed.status, "PR created");
    }

    #[test]
    fn legacy_nested_exhaustion_keeps_only_latest_actionable_context() {
        let legacy = "Automatic correction limit. Correction history: Automatic correction limit. Correction history:\nAttempt 1 (report): missing field verification\nAttempt 6 (report): latest missing field status\nSELF-REPAIR REQUIRED: old instruction";
        let context = resume_failure_context(legacy);
        assert!(context.contains("latest missing field status"));
        assert!(!context.contains("exhaust"));
        assert!(!context.contains("Correction history"));
        assert!(!context.contains("old instruction"));
        assert!(!context.contains("missing field verification"));
    }

    #[test]
    fn publication_failure_retries_without_reimplementing() {
        let s = Sandbox::new();
        let calls = Arc::new(AtomicUsize::new(0));
        fs::write(s.root.join("offline"), "").unwrap();
        assert!(s.run("complete", calls.clone()).is_err());
        assert!(load(&s.repo, &s.ticket).unwrap().verified_head.is_some());
        fs::remove_file(s.root.join("offline")).unwrap();
        assert_eq!(
            s.run("complete", calls.clone()).unwrap().status,
            "PR created"
        );
        assert_eq!(calls.load(Ordering::SeqCst), 1);
    }
    #[test]
    fn changed_ticket_and_concurrent_run_are_rejected() {
        let s = Sandbox::new();
        let calls = Arc::new(AtomicUsize::new(0));
        assert!(s.run("cancel", calls.clone()).is_err());
        let dir = state_dir(&s.repo, &s.ticket).unwrap();
        let lock = fs::OpenOptions::new()
            .read(true)
            .write(true)
            .open(dir.join("run.lock"))
            .unwrap();
        lock.lock().unwrap();
        assert!(
            s.run("complete", calls.clone())
                .unwrap_err()
                .to_string()
                .contains("already being implemented")
        );
        drop(lock);
        fs::write(s.repo.join(&s.ticket), "# Different scope").unwrap();
        assert!(
            s.run("complete", calls.clone())
                .unwrap_err()
                .to_string()
                .contains("Ticket changed")
        );
        assert_eq!(calls.load(Ordering::SeqCst), 1);
    }
    #[test]
    fn mismatched_existing_worktree_is_not_modified() {
        let s = Sandbox::new();
        let calls = Arc::new(AtomicUsize::new(0));
        assert!(s.run("cancel", calls.clone()).is_err());
        let mut state = load(&s.repo, &s.ticket).unwrap();
        state.worktree = s.repo.clone();
        save(&state_dir(&s.repo, &s.ticket).unwrap(), &state).unwrap();
        assert!(
            s.run("complete", calls.clone())
                .unwrap_err()
                .to_string()
                .contains("another branch")
        );
        assert!(!s.repo.join("implemented.txt").exists());
        assert_eq!(calls.load(Ordering::SeqCst), 1);
    }

    fn cleanup_runner() -> Runner {
        let (progress, _rx) = mpsc::channel();
        Runner { gh: "unused".into(), deadline: Instant::now() + Duration::from_secs(30),
            cancel: Arc::new(AtomicBool::new(false)), progress }
    }

    // Produce a completed record without invoking automatic cleanup: models an
    // older Packet version leaving a merged PR's worktree behind.
    fn completed_cleanup_fixture(s: &Sandbox) -> Implementation {
        let mut state = s.run("complete", Arc::new(AtomicUsize::new(0))).unwrap();
        let head = state.verified_head.as_deref().unwrap();
        s.git(&s.repo, &["push", "origin", &format!("{head}:refs/heads/main")]);
        state.status = "Done".into();
        state.pr_state = Some("MERGED".into());
        state.merged_commit = state.verified_head.clone();
        save(&state_dir(&s.repo, &s.ticket).unwrap(), &state).unwrap();
        state
    }

    #[test]
    fn relocated_completed_tickets_keep_board_state_and_original_evidence_identity() {
        for legacy_storage in [false, true] {
            let s = Sandbox::new();
            let done = completed_cleanup_fixture(&s);
            let original_dir = state_dir(&s.repo, &s.ticket).unwrap();
            let storage = if legacy_storage {
                let legacy = legacy_state_dir(&s.repo, &s.ticket).unwrap();
                fs::create_dir_all(legacy.parent().unwrap()).unwrap();
                fs::rename(&original_dir, &legacy).unwrap();
                legacy
            } else { original_dir };
            let before = fs::read(storage.join("state.json")).unwrap();
            let relocated = format!(".kool-ade-packet/{}", s.ticket);
            fs::create_dir_all(s.repo.join(&relocated).parent().unwrap()).unwrap();
            fs::rename(s.repo.join(&s.ticket), s.repo.join(&relocated)).unwrap();

            let board = load_board_states(&s.repo);
            let record = board.get(&relocated).expect("relocated task remains completed");
            assert_eq!(record.status, "Done");
            assert_eq!(record.ticket, s.ticket, "cleanup and evidence keep original identity");
            assert_eq!(record.merged_commit, done.merged_commit);
            assert_eq!(board_column(Some(record), false), 4);
            let docs = vec![crate::artifacts::task_docs::TaskDocument {
                path: relocated.clone(), title: "Moved completed task".into(),
                text: done.ticket_text.clone(),
            }];
            assert!(crate::core::implementation_queue::next_ready_ticket(
                &docs, &board, &Default::default()
            ).unwrap().is_none(), "completed work must not be reimplemented");
            assert_eq!(load(&s.repo, &relocated).unwrap().status, "Done");
            assert_eq!(fs::read(storage.join("state.json")).unwrap(), before);
            refresh_pr(&s.repo, &relocated, &cleanup_runner()).unwrap();
            assert!(load(&s.repo, &relocated).unwrap().cleanup.completed_at.is_some(),
                "relocated lookup retains the original cleanup ownership identity");

            fs::write(s.repo.join(&relocated), "# Revised task contract").unwrap();
            assert!(!load_board_states(&s.repo).contains_key(&relocated));
            assert!(load(&s.repo, &relocated).is_none(), "revised work cannot inherit Done");
            fs::write(s.repo.join(&relocated), &done.ticket_text).unwrap();
            fs::write(s.repo.join(&s.ticket), &done.ticket_text).unwrap();
            assert!(!load_board_states(&s.repo).contains_key(&relocated), "two existing tasks are distinct");
        }
    }

    #[test]
    fn rewritten_ticket_path_in_old_state_directory_resumes_original_worktree() {
        for legacy_storage in [false, true] {
            let mut s = Sandbox::new();
            let calls = Arc::new(AtomicUsize::new(0));
            assert!(s.run("cancel", calls.clone()).is_err());
            let original_dir = state_dir(&s.repo, &s.ticket).unwrap();
            let mut state = load(&s.repo, &s.ticket).unwrap();
            let storage = if legacy_storage {
                let legacy = legacy_state_dir(&s.repo, &s.ticket).unwrap();
                fs::create_dir_all(legacy.parent().unwrap()).unwrap();
                fs::rename(&original_dir, &legacy).unwrap();
                legacy
            } else { original_dir };
            let relocated = format!(".kool-ade-packet/{}", s.ticket);
            fs::create_dir_all(s.repo.join(&relocated).parent().unwrap()).unwrap();
            fs::rename(s.repo.join(&s.ticket), s.repo.join(&relocated)).unwrap();
            // Reproduce the real partial migration: JSON was rewritten but
            // its containing directory, branch, and worktree kept old hashes.
            state.ticket = relocated.clone();
            save(&storage, &state).unwrap();
            let before = fs::read(storage.join("state.json")).unwrap();
            assert_eq!(load(&s.repo, &relocated).unwrap().worktree, state.worktree);
            assert_eq!(fs::read(storage.join("state.json")).unwrap(), before);
            s.ticket = relocated;
            let resumed = s.run("resume", calls.clone()).unwrap();
            assert_eq!(resumed.worktree, state.worktree);
            assert_eq!(resumed.branch, state.branch);
            assert_eq!(resumed.base_commit, state.base_commit);
            assert_eq!(calls.load(Ordering::SeqCst), 2);
        }
    }

    #[test]
    fn rewritten_dependency_record_is_loaded_from_original_directory() {
        let s = Sandbox::new();
        let mut state = completed_cleanup_fixture(&s);
        let storage = state_dir(&s.repo, &s.ticket).unwrap();
        let relocated = format!(".kool-ade-packet/{}", s.ticket);
        fs::create_dir_all(s.repo.join(&relocated).parent().unwrap()).unwrap();
        fs::rename(s.repo.join(&s.ticket), s.repo.join(&relocated)).unwrap();
        state.ticket = relocated.clone();
        save(&storage, &state).unwrap();
        let ticket = ".kool-ade-packet/planning/tasks/feature/CHG-003-TASK-verify.md";
        let text = "## Dependencies\n- [Completed work](001-implement-ticket-behavior.md)\n";
        let context = completed_dependency_context(&s.repo, ticket, text).unwrap().unwrap();
        assert!(context.contains(state.merged_commit.as_deref().unwrap()));
        fs::write(s.repo.join(&relocated), "# Changed contract").unwrap();
        assert!(completed_dependency_context(&s.repo, ticket, text).is_err());
    }

    #[test]
    fn cleanup_reclaims_ignored_builds_keeps_evidence_and_is_idempotent() {
        let s = Sandbox::new();
        let state = completed_cleanup_fixture(&s);
        fs::write(common(&s.repo).unwrap().join("info/exclude"), "target/\n").unwrap();
        fs::create_dir_all(state.worktree.join("target/debug")).unwrap();
        fs::write(state.worktree.join("target/debug/build-cache"), vec![0u8; 1024 * 1024]).unwrap();
        let dir = state_dir(&s.repo, &s.ticket).unwrap();
        let evidence = fs::read(dir.join("verified-report.json")).unwrap();
        refresh_pr(&s.repo, &s.ticket, &cleanup_runner()).unwrap();
        let done = load(&s.repo, &s.ticket).unwrap();
        assert!(done.cleanup.completed_at.is_some(), "{:?}", done.cleanup);
        assert!(!state.worktree.exists());
        assert_eq!(fs::read(dir.join("verified-report.json")).unwrap(), evidence);
        assert_eq!(done.status, "Done");
        assert_eq!(board_column(Some(&done), false), 4);
        refresh_pr(&s.repo, &s.ticket, &cleanup_runner()).unwrap();
        assert_eq!(load(&s.repo, &s.ticket).unwrap().cleanup, done.cleanup);
    }

    #[test]
    fn cleanup_of_merged_pr_and_legacy_completed_state_survives_restart() {
        let s = Sandbox::new();
        let mut state = completed_cleanup_fixture(&s);
        state.status = "PR created".into();
        state.pr_state = Some("OPEN".into());
        save(&state_dir(&s.repo, &s.ticket).unwrap(), &state).unwrap();
        fs::write(&s.gh, format!("#!/bin/sh\nprintf '%s\\n' '{{\"state\":\"MERGED\",\"mergeCommit\":{{\"oid\":\"{}\"}}}}'\n", state.merged_commit.as_deref().unwrap())).unwrap();
        let current = state_dir(&s.repo, &s.ticket).unwrap();
        let legacy = legacy_state_dir(&s.repo, &s.ticket).unwrap();
        fs::create_dir_all(legacy.parent().unwrap()).unwrap();
        fs::rename(&current, &legacy).unwrap();
        fs::remove_file(legacy.join("run.lock")).unwrap();
        let mut runner = cleanup_runner();
        runner.gh = s.gh.to_string_lossy().into();
        refresh_pr(&s.repo, &s.ticket, &runner).unwrap();
        let done = load(&s.repo, &s.ticket).unwrap();
        assert_eq!(done.status, "Done");
        assert!(done.cleanup.completed_at.is_some(), "{:?}", done.cleanup);
        assert!(!state.worktree.exists());
        assert!(legacy.join("verified-report.json").exists());
    }

    #[test]
    fn cleanup_never_reclaims_an_unpublished_completion_commit() {
        let s = Sandbox::new();
        let mut state = s.run("complete", Arc::new(AtomicUsize::new(0))).unwrap();
        state.status = "Done".into();
        state.merged_commit = state.verified_head.clone();
        save(&state_dir(&s.repo, &s.ticket).unwrap(), &state).unwrap();
        refresh_pr(&s.repo, &s.ticket, &cleanup_runner()).unwrap();
        let preserved = load(&s.repo, &s.ticket).unwrap();
        assert!(preserved.cleanup.error.unwrap().contains("not in origin"));
        assert!(state.worktree.join("implemented.txt").exists());
    }

    #[test]
    fn cleanup_preserves_changes_and_retries_after_they_are_resolved() {
        let s = Sandbox::new();
        let state = completed_cleanup_fixture(&s);
        let draft = state.worktree.join("unsaved-draft.txt");
        fs::write(&draft, "keep this").unwrap();
        refresh_pr(&s.repo, &s.ticket, &cleanup_runner()).unwrap();
        let failed = load(&s.repo, &s.ticket).unwrap();
        assert!(failed.cleanup.error.as_deref().unwrap().contains("local changes"));
        assert_eq!(fs::read_to_string(&draft).unwrap(), "keep this");
        assert_eq!(failed.status, "Done");
        fs::rename(&draft, s.root.join("saved-draft.txt")).unwrap();
        refresh_pr(&s.repo, &s.ticket, &cleanup_runner()).unwrap();
        assert!(load(&s.repo, &s.ticket).unwrap().cleanup.completed_at.is_some());
        assert!(!state.worktree.exists());
    }

    #[test]
    fn cleanup_preserves_changed_head_and_locked_worktree() {
        let s = Sandbox::new();
        let state = completed_cleanup_fixture(&s);
        s.git(&s.repo, &["worktree", "lock", state.worktree.to_str().unwrap()]);
        refresh_pr(&s.repo, &s.ticket, &cleanup_runner()).unwrap();
        assert!(load(&s.repo, &s.ticket).unwrap().cleanup.error.is_some());
        assert!(state.worktree.exists());
        s.git(&s.repo, &["worktree", "unlock", state.worktree.to_str().unwrap()]);
        s.git(&state.worktree, &["commit", "--allow-empty", "-qm", "new local work"]);
        refresh_pr(&s.repo, &s.ticket, &cleanup_runner()).unwrap();
        assert!(load(&s.repo, &s.ticket).unwrap().cleanup.error.unwrap().contains("changed HEAD"));
        assert!(state.worktree.exists());
    }

    #[test]
    fn cleanup_preserves_wrong_identity_missing_publication_and_active_work() {
        let s = Sandbox::new();
        let mut state = completed_cleanup_fixture(&s);
        let dir = state_dir(&s.repo, &s.ticket).unwrap();
        let lock = fs::OpenOptions::new().read(true).write(true).open(dir.join("run.lock")).unwrap();
        lock.lock().unwrap();
        refresh_pr(&s.repo, &s.ticket, &cleanup_runner()).unwrap();
        assert!(state.worktree.exists());
        assert!(load(&s.repo, &s.ticket).unwrap().cleanup.attempted_at.is_none());
        drop(lock);
        state.worktree = s.repo.clone();
        save(&dir, &state).unwrap();
        refresh_pr(&s.repo, &s.ticket, &cleanup_runner()).unwrap();
        assert!(load(&s.repo, &s.ticket).unwrap().cleanup.error.unwrap().contains("allocation"));
        assert!(s.repo.join(&s.ticket).exists());
        state.merged_commit = None;
        save(&dir, &state).unwrap();
        refresh_pr(&s.repo, &s.ticket, &cleanup_runner()).unwrap();
        assert!(load(&s.repo, &s.ticket).unwrap().cleanup.error.unwrap().contains("No confirmed"));
    }

    #[test]
    fn publication_targets_the_origin_repository() {
        assert_eq!(
            remote_repository("git@github.com:team/project.git"),
            "github.com/team/project"
        );
        assert_eq!(
            remote_repository("https://github.com/team/project.git"),
            "github.com/team/project"
        );
        assert_eq!(
            remote_repository("ssh://git@github.company.test/team/project.git"),
            "github.company.test/team/project"
        );
    }

    #[test]
    fn verification_receives_corrections_even_after_report_retries_are_used() {
        let s = Sandbox::new();
        let calls = Arc::new(AtomicUsize::new(0));
        let state = s.run("repair_mixed", calls.clone()).unwrap();
        assert_eq!(calls.load(Ordering::SeqCst), 5);
        assert_eq!(state.status, "PR created");
        assert!(state.worktree.join("missing-file").exists());
    }

    #[test]
    fn verification_worktree_path_survives_cd_and_spaces() {
        let s = Sandbox::new();
        let cwd = s.root.join("worktree with spaces");
        fs::create_dir(&cwd).unwrap();
        fs::write(cwd.join("marker"), "proof").unwrap();
        let (progress, _rx) = mpsc::channel();
        let runner = Runner {
            gh: "unused".into(),
            deadline: Instant::now() + Duration::from_secs(5),
            cancel: Arc::new(AtomicBool::new(false)),
            progress,
        };
        runner
            .verify(
                &cwd,
                "cd / && test \"$(cat \"$PACKET_WORKTREE/marker\")\" = proof",
            )
            .unwrap();
        runner.verify(&cwd, "test -f marker && test -z \"${PREVIOUS_CHECK_VARIABLE+x}\" && PREVIOUS_CHECK_VARIABLE=value").unwrap();
        runner
            .verify(&cwd, "test -z \"${PREVIOUS_CHECK_VARIABLE+x}\"")
            .unwrap();
    }

    #[test]
    fn failed_command_retains_both_streams_for_correction() {
        let (progress, _rx) = mpsc::channel();
        let runner = Runner {
            gh: "unused".into(),
            deadline: Instant::now() + Duration::from_secs(5),
            cancel: Arc::new(AtomicBool::new(false)),
            progress,
        };
        let error = runner
            .command(
                &std::env::temp_dir(),
                "/bin/sh",
                &["-c", "echo assertion-detail; echo diagnostic >&2; exit 1"],
            )
            .unwrap_err()
            .to_string();
        assert!(error.contains("assertion-detail"));
        assert!(error.contains("diagnostic"));
    }

    #[test]
    fn harness_failures_retry_and_sidecar_survives_lost_final_message() {
        for (mode, expected) in [("harness_retry", 2), ("sidecar", 1), ("healing", 5)] {
            let s = Sandbox::new();
            let calls = Arc::new(AtomicUsize::new(0));
            let result = s.run(mode, calls.clone()).unwrap();
            assert_eq!(result.status, "PR created");
            assert_eq!(calls.load(Ordering::SeqCst), expected);
            assert_eq!(
                s.git(
                    &result.worktree,
                    &[
                        "rev-list",
                        "--count",
                        &format!("{}..HEAD", result.base_commit)
                    ]
                ),
                "1"
            );
        }
        let s = Sandbox::new();
        let calls = Arc::new(AtomicUsize::new(0));
        assert!(
            s.run("harness_dead", calls.clone())
                .unwrap_err()
                .to_string()
                .contains("Harness recovery exhausted")
        );
        assert_eq!(calls.load(Ordering::SeqCst), 6);
        assert!(!s.root.join("pr-created").exists());
    }

    #[test]
    fn auto_mode_integrates_atomically_without_gh_and_recovers_a_lost_push_response() {
        let s = Sandbox::new();
        let original = s.git(&s.repo, &["rev-parse", "HEAD"]);
        fs::write(s.repo.join("draft.txt"), "preserve this draft").unwrap();
        let calls = Arc::new(AtomicUsize::new(0));
        let run = || {
            let (tx, _rx) = mpsc::channel();
            run_with_options(
                &s.repo,
                &s.ticket,
                &Fixture {
                    mode: "complete",
                    calls: calls.clone(),
                },
                Arc::new(AtomicBool::new(false)),
                tx,
                "must-not-invoke-gh",
                true,
            )
        };
        let mut result = run().unwrap();
        assert_eq!(result.status, "Done");
        assert!(result.pr_url.is_none());
        assert!(result.cleanup.completed_at.is_some(), "{:?}", result.cleanup);
        assert!(!result.worktree.exists());
        let worktrees = s.git(&s.repo, &["worktree", "list", "--porcelain"]);
        assert_eq!(worktrees.lines().filter(|line| line.starts_with("worktree ")).count(), 1);
        let merged = result.merged_commit.clone().unwrap();
        let remote = s.root.join("remote.git");
        assert_eq!(s.git(&remote, &["rev-parse", "refs/heads/main"]), merged);
        assert_eq!(
            s.git(
                &remote,
                &[
                    "rev-list",
                    "--count",
                    &format!("{original}..refs/heads/main")
                ]
            ),
            "1"
        );
        assert_eq!(
            s.git(&remote, &["show", "main:implemented.txt"]),
            "implemented"
        );
        assert_eq!(s.git(&s.repo, &["rev-parse", "HEAD"]), original);
        assert_eq!(
            fs::read_to_string(s.repo.join("draft.txt")).unwrap(),
            "preserve this draft"
        );
        result.status = "Publishing".into();
        save(&state_dir(&s.repo, &s.ticket).unwrap(), &result).unwrap();
        assert_eq!(run().unwrap().status, "Done");
        assert_eq!(calls.load(Ordering::SeqCst), 1);
        assert!(!s.root.join("pr-created").exists());
    }

    #[test]
    fn auto_mode_starts_on_remote_when_local_history_diverged() {
        let s = Sandbox::new();
        let remote = s.advance_remote();
        fs::write(s.repo.join("local-only.txt"), "local work").unwrap();
        s.git(&s.repo, &["add", "local-only.txt"]);
        s.git(&s.repo, &["commit", "-qm", "local work"]);
        let local = s.git(&s.repo, &["rev-parse", "HEAD"]);
        let (tx, _rx) = mpsc::channel();
        let result = run_with_options(
            &s.repo,
            &s.ticket,
            &Fixture {
                mode: "complete",
                calls: Arc::new(AtomicUsize::new(0)),
            },
            Arc::new(AtomicBool::new(false)),
            tx,
            "must-not-run-gh",
            true,
        )
        .unwrap();
        assert_eq!(result.status, "Done");
        assert_eq!(result.base_commit, remote);
        assert_eq!(s.git(&s.repo, &["rev-parse", "HEAD"]), local);
        assert_eq!(
            fs::read_to_string(s.repo.join("local-only.txt")).unwrap(),
            "local work"
        );
        assert!(!result.worktree.join("local-only.txt").exists());
    }

    #[test]
    fn auto_mode_includes_remote_changes_that_arrive_during_implementation() {
        struct Advancing<'a> {
            sandbox: &'a Sandbox,
            calls: Arc<AtomicUsize>,
        }
        impl AiHarness for Advancing<'_> {
            fn label(&self) -> String {
                "advancing fixture".into()
            }
            fn check_available(&self) -> Result<String, AppError> {
                Ok("fixture".into())
            }
            fn execute(
                &self,
                req: &PlanningRequest,
            ) -> Result<crate::harness::HarnessOutcome, AppError> {
                self.sandbox.advance_remote();
                fs::write(
                    self.sandbox.repo.join("local-only.txt"),
                    "preserved local work",
                )
                .unwrap();
                self.sandbox
                    .git(&self.sandbox.repo, &["add", "local-only.txt"]);
                self.sandbox.git(
                    &self.sandbox.repo,
                    &["commit", "-qm", "local work during implementation"],
                );
                Fixture {
                    mode: "complete",
                    calls: self.calls.clone(),
                }
                .execute(req)
            }
        }
        let s = Sandbox::new();
        let calls = Arc::new(AtomicUsize::new(0));
        let (tx, _rx) = mpsc::channel();
        let result = run_with_options(
            &s.repo,
            &s.ticket,
            &Advancing {
                sandbox: &s,
                calls: calls.clone(),
            },
            Arc::new(AtomicBool::new(false)),
            tx,
            "must-not-run-gh",
            true,
        )
        .unwrap();
        assert_eq!(result.status, "Done");
        assert_eq!(calls.load(Ordering::SeqCst), 1);
        assert_eq!(
            fs::read_to_string(s.repo.join("local-only.txt")).unwrap(),
            "preserved local work"
        );
        assert!(
            s.git(&s.repo, &["log", "-1", "--format=%s"])
                .contains("local work during implementation")
        );
        assert_eq!(
            s.git(&s.root.join("remote.git"), &["show", "main:upstream.txt"]),
            "latest upstream"
        );
        assert_eq!(
            s.git(
                &s.root.join("remote.git"),
                &["show", "main:implemented.txt"]
            ),
            "implemented"
        );
    }

    #[test]
    fn auto_mode_repairs_a_conflicted_integration_before_one_atomic_main_commit() {
        struct Conflicting<'a> {
            sandbox: &'a Sandbox,
            calls: Arc<AtomicUsize>,
        }
        impl AiHarness for Conflicting<'_> {
            fn label(&self) -> String {
                "conflicting fixture".into()
            }
            fn check_available(&self) -> Result<String, AppError> {
                Ok("fixture".into())
            }
            fn execute(
                &self,
                req: &PlanningRequest,
            ) -> Result<crate::harness::HarnessOutcome, AppError> {
                if self.calls.load(Ordering::SeqCst) == 0 {
                    self.sandbox.advance_remote();
                    let peer = self.sandbox.root.join("peer");
                    fs::write(
                        peer.join("implemented.txt"),
                        "concurrent upstream implementation",
                    )
                    .unwrap();
                    self.sandbox.git(&peer, &["add", "."]);
                    self.sandbox
                        .git(&peer, &["commit", "-qm", "concurrent change"]);
                    self.sandbox.git(&peer, &["push", "-q", "origin", "main"]);
                } else {
                    assert!(req.prompt_body.contains("Integration verification failure"));
                    assert!(req.prompt_body.contains("merge conflicts"));
                }
                Fixture {
                    mode: "complete",
                    calls: self.calls.clone(),
                }
                .execute(req)
            }
        }
        let s = Sandbox::new();
        let calls = Arc::new(AtomicUsize::new(0));
        let (tx, _rx) = mpsc::channel();
        let result = run_with_options(
            &s.repo,
            &s.ticket,
            &Conflicting {
                sandbox: &s,
                calls: calls.clone(),
            },
            Arc::new(AtomicBool::new(false)),
            tx,
            "must-not-run",
            true,
        )
        .unwrap();
        assert_eq!(result.status, "Done");
        assert_eq!(calls.load(Ordering::SeqCst), 2);
        let remote = s.root.join("remote.git");
        assert_eq!(s.git(&remote, &["rev-list", "--count", "main"]), "4");
        assert_eq!(
            s.git(&remote, &["show", "main:implemented.txt"]),
            "implemented"
        );
        assert_eq!(
            s.git(&remote, &["show", "main:upstream.txt"]),
            "latest upstream"
        );
    }

    #[test]
    fn failed_auto_verification_never_changes_main() {
        let s = Sandbox::new();
        let initial = s.git(&s.root.join("remote.git"), &["rev-parse", "main"]);
        let (tx, _rx) = mpsc::channel();
        assert!(
            run_with_options(
                &s.repo,
                &s.ticket,
                &Fixture {
                    mode: "fail",
                    calls: Arc::new(AtomicUsize::new(0))
                },
                Arc::new(AtomicBool::new(false)),
                tx,
                "must-not-run",
                true
            )
            .is_err()
        );
        assert_eq!(
            s.git(&s.root.join("remote.git"), &["rev-parse", "main"]),
            initial
        );
    }

    #[test]
    fn incomplete_acceptance_evidence_fails_closed() {
        let report = Report {
            status: "complete".into(),
            summary: "Done".into(),
            acceptance_criteria: vec![Criterion {
                criterion: "One".into(),
                evidence: "Proof".into(),
            }],
            verification: vec!["cargo test".into()],
            remaining: vec![],
        };
        assert!(validate_report(&report, "## Acceptance criteria\n- One\n- Two\n").is_err());
    }
}

/// Task-only activity survives reconnects without becoming a tracked artifact.
pub fn load_activity(repo: &Path, ticket: &str) -> Option<crate::harness::LiveProgress> {
    serde_json::from_slice(&fs::read(state_dir(repo, ticket).ok()?.join("activity.json")).ok()?)
        .ok()
}
pub fn save_activity(
    repo: &Path,
    ticket: &str,
    activity: &crate::harness::LiveProgress,
) -> anyhow::Result<()> {
    let dir = state_dir(repo, ticket)?;
    fs::create_dir_all(&dir)?;
    let temp = dir.join("activity.json.tmp");
    // Persist a bounded view: in-memory snapshots keep the full history, but
    // the on-disk snapshot keeps only trailing windows so a long turn cannot
    // bloat activity.json (quadratic rewrites of ever-larger files were a
    // major source of disk wear).
    fs::write(&temp, serde_json::to_vec(&trim_for_persist(activity))?)?;
    fs::rename(temp, dir.join("activity.json"))?;
    Ok(())
}

/// Bounds for the persisted activity snapshot (see [`save_activity`]).
const ACTIVITY_MAX_POSTS: usize = 200;
const ACTIVITY_MAX_POST_CHARS: usize = 2_000;
const ACTIVITY_MAX_FIELD_CHARS: usize = 32_000;

/// Build the bounded, persistence-shaped copy of a live snapshot.
/// Pure: the caller's in-memory state is never mutated.
fn trim_for_persist(progress: &crate::harness::LiveProgress) -> crate::harness::LiveProgress {
    let mut out = progress.clone();
    if out.posts.len() > ACTIVITY_MAX_POSTS {
        let drop = out.posts.len() - ACTIVITY_MAX_POSTS;
        out.posts.drain(..drop);
    }
    for post in &mut out.posts {
        post.text = retain_suffix(&post.text, ACTIVITY_MAX_POST_CHARS);
    }
    out.thoughts = retain_suffix(&out.thoughts, ACTIVITY_MAX_FIELD_CHARS);
    out.response = retain_suffix(&out.response, ACTIVITY_MAX_FIELD_CHARS);
    out.specification = out
        .specification
        .as_deref()
        .map(|s| retain_suffix(s, ACTIVITY_MAX_FIELD_CHARS));
    out.activity = out
        .activity
        .as_deref()
        .map(|s| retain_suffix(s, ACTIVITY_MAX_FIELD_CHARS));
    out
}

/// Keep the LAST `max_chars` characters of `s` (the tail carries the newest
/// content), prefixing an elision marker when anything was dropped.
/// Character-boundary safe.
fn retain_suffix(s: &str, max_chars: usize) -> String {
    let total = s.chars().count();
    if total <= max_chars {
        return s.to_owned();
    }
    let drop = total - max_chars;
    let cut = s.char_indices().nth(drop).map(|(idx, _)| idx).unwrap_or(0);
    format!("\u{2026}{}", &s[cut..])
}

#[cfg(test)]
mod activity_persist_trims {
    use super::*;
    use crate::harness::LivePost;

    fn post(id: u64, text: String) -> LivePost {
        LivePost {
            id: (id, 0),
            kind: "assistant_message".into(),
            text,
        }
    }

    #[test]
    fn suffix_keep_is_char_safe_and_marked() {
        // Multibyte content must never panic at a char boundary.
        let s = "あ".repeat(10);
        let kept = retain_suffix(&s, 4);
        assert_eq!(kept, format!("\u{2026}{}", "あ".repeat(4)));
        // Under the cap: unchanged, no marker.
        assert_eq!(retain_suffix("hello world", 100), "hello world");
        // Exact cap: unchanged.
        assert_eq!(retain_suffix("abcde", 5), "abcde");
        // Keeps the NEWEST tail, drops the head.
        assert_eq!(retain_suffix("0123456789", 4), format!("\u{2026}6789"));
    }

    #[test]
    fn trim_bounds_posts_and_fields_without_touching_input() {
        let big_post = "x".repeat(ACTIVITY_MAX_POST_CHARS * 3);
        let mut big = crate::harness::LiveProgress::default();
        for i in 0..(ACTIVITY_MAX_POSTS as u64 + 50) {
            big.posts.push(post(i, "line".repeat(100)));
        }
        big.posts.push(post(999_999, big_post.clone()));
        big.thoughts = "t".repeat(ACTIVITY_MAX_FIELD_CHARS * 2);
        big.response = "r".repeat(ACTIVITY_MAX_FIELD_CHARS * 2);
        big.specification = Some("s".repeat(ACTIVITY_MAX_FIELD_CHARS * 2));
        big.activity = Some("working".to_string());
        let before_posts = big.posts.len();
        let before_response = big.response.len();

        let trimmed = trim_for_persist(&big);

        assert_eq!(trimmed.posts.len(), ACTIVITY_MAX_POSTS);
        assert!(trimmed.posts.len() < before_posts);
        // Oldest 51 posts dropped (251 total -> 200 kept); newest kept.
        assert_eq!(trimmed.posts[0].id.0, 51);
        // Oversized post text bounded, tail preserved.
        let last = trimmed.posts.last().unwrap();
        assert!(last.text.chars().count() <= ACTIVITY_MAX_POST_CHARS + 1);
        assert!(
            last.text
                .ends_with(&"x".repeat(ACTIVITY_MAX_POST_CHARS.min(10)))
        );
        // Long fields bounded to tail-with-marker.
        for field in [
            &trimmed.thoughts,
            &trimmed.response,
            trimmed.specification.as_deref().unwrap(),
            trimmed.activity.as_deref().unwrap(),
        ] {
            assert!(field.chars().count() <= ACTIVITY_MAX_FIELD_CHARS + 1);
            assert!(
                field.starts_with('\u{2026}') || field.chars().count() <= ACTIVITY_MAX_FIELD_CHARS
            );
        }
        // Input snapshot untouched.
        assert_eq!(big.posts.len(), before_posts);
        assert_eq!(big.response.len(), before_response);
        // Round-trips through JSON the way save_activity writes it.
        let wire = serde_json::to_vec(&trimmed).unwrap();
        let back: crate::harness::LiveProgress = serde_json::from_slice(&wire).unwrap();
        assert_eq!(back, trimmed);
    }
}
