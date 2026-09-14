//! Resumable ticket implementation. Git worktrees and runtime records are kept
//! independently from planning state; only verified results proceed to a PR.
use crate::harness::{AiHarness, LiveProgress, PiHarness, PlanningRequest};
use serde::{Deserialize, Serialize};
use std::{
    fs,
    path::{Path, PathBuf},
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
        mpsc::{self, Receiver, Sender},
    },
    time::{Duration, Instant},
};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Implementation {
    pub ticket: String,
    pub ticket_text: String,
    #[serde(default)]
    pub approved_specification: Option<String>,
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
}
#[derive(Debug, Deserialize, Serialize)]
struct Report {
    status: String,
    summary: String,
    acceptance_criteria: Vec<Criterion>,
    verification: Vec<String>,
    remaining: Vec<String>,
}
#[derive(Debug, Deserialize, Serialize)]
struct Criterion {
    criterion: String,
    evidence: String,
}

pub enum Event {
    Progress(LiveProgress),
    Done(Result<Implementation, String>),
}
pub struct Controller {
    rx: Receiver<Event>,
    cancel: Arc<AtomicBool>,
}
impl Controller {
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
            let _ = tx.send(Event::Done(result.map_err(|e| e.to_string())));
        });
        Self { rx, cancel }
    }
    pub fn poll(&self) -> Option<Event> {
        self.rx.try_recv().ok()
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
                return Err(e);
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
    fn verify(&self, cwd: &Path, command: &str) -> anyhow::Result<String> {
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
    Ok(common(repo)?
        .join("packet-implementations")
        .join(key(ticket)))
}
pub fn load_all(repo: &Path) -> Vec<Implementation> {
    let Ok(common) = common(repo) else {
        return Vec::new();
    };
    let Ok(entries) = fs::read_dir(common.join("packet-implementations")) else {
        return Vec::new();
    };
    entries
        .filter_map(Result::ok)
        .filter_map(|entry| {
            serde_json::from_slice(&fs::read(entry.path().join("state.json")).ok()?).ok()
        })
        .collect()
}
pub fn load(repo: &Path, ticket: &str) -> Option<Implementation> {
    serde_json::from_slice(&fs::read(state_dir(repo, ticket).ok()?.join("state.json")).ok()?).ok()
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
    rx: Receiver<()>,
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
            for ticket in tickets {
                if runner.remaining().is_err() {
                    break;
                }
                let _ = refresh_pr(&repo, &ticket, &runner);
            }
            let _ = tx.send(());
        });
        Self { rx, cancel }
    }
    pub fn finished(&self) -> bool {
        !matches!(self.rx.try_recv(), Err(mpsc::TryRecvError::Empty))
    }
}
impl Drop for PrRefresh {
    fn drop(&mut self) {
        self.cancel.store(true, Ordering::SeqCst);
    }
}

fn refresh_pr(repo: &Path, ticket: &str, runner: &Runner) -> anyhow::Result<()> {
    let dir = state_dir(repo, ticket)?;
    let lock = fs::OpenOptions::new()
        .read(true)
        .write(true)
        .open(dir.join("run.lock"))?;
    if lock.try_lock().is_err() {
        return Ok(());
    }
    let mut state: Implementation = serde_json::from_slice(&fs::read(dir.join("state.json"))?)?;
    let target_repo = target_repository(repo, ticket)?;
    let Some(url) = state.pr_url.clone() else {
        return Ok(());
    };
    if state.pr_state.as_deref() == Some("MERGED") {
        return Ok(());
    }
    state.pr_check_attempted_at = Some(chrono::Utc::now().to_rfc3339());
    let result = (|| -> anyhow::Result<String> {
        let output = runner.command(
            &target_repo,
            &runner.gh,
            &["pr", "view", &url, "--json", "state"],
        )?;
        let value: serde_json::Value = serde_json::from_str(&output)?;
        let status = value["state"].as_str().unwrap_or_default();
        anyhow::ensure!(
            matches!(status, "OPEN" | "CLOSED" | "MERGED"),
            "GitHub returned an unknown PR state"
        );
        Ok(status.to_owned())
    })();
    match result {
        Ok(status) => {
            state.status = match status.as_str() {
                "MERGED" => "Done",
                "CLOSED" => "PR closed",
                _ => "PR created",
            }
            .into();
            state.pr_state = Some(status);
            state.pr_checked_at = Some(chrono::Utc::now().to_rfc3339());
            state.pr_check_error = None;
        }
        Err(error) => state.pr_check_error = Some(error.to_string()),
    }
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
        "Preparing" | "Implementing" | "Verifying" | "Ready for PR" | "Publishing" if busy => 1,
        _ => 3,
    }
}
fn read_ticket(repo: &Path, ticket: &str) -> anyhow::Result<String> {
    anyhow::ensure!(
        ticket.starts_with("planning/tasks/")
            && ticket.ends_with(".md")
            && Path::new(ticket)
                .file_name()
                .and_then(|n| n.to_str())
                .is_some_and(|n| n.starts_with(|c: char| c.is_ascii_digit())),
        "Select a numbered task story"
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
            state.ticket == ticket && state.ticket_text == text,
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
        let head = if auto_merge {
            remote.clone()
        } else if runner
            .git(repo, &["merge-base", "--is-ancestor", &local, &remote])
            .is_ok()
        {
            remote
        } else {
            runner.git(repo, &["merge-base", "--is-ancestor", &remote, &local])
                .map_err(|_| anyhow::anyhow!("Local {base} and origin/{base} have diverged. Reconcile the branch before implementing; no work was discarded."))?;
            local
        };
        let root = repo
            .parent()
            .ok_or_else(|| anyhow::anyhow!("Repository has no parent"))?
            .join(".packet-worktrees")
            .join(crate::persistence::project_slug(&repo.canonicalize()?));
        fs::create_dir_all(&root)?;
        Implementation {
            ticket: ticket.into(),
            ticket_text: text,
            approved_specification: Path::new(ticket).parent().and_then(|parent| {
                fs::read_to_string(planning_root.join(parent).join("specification.md")).ok()
            }),
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
        }
    };
    if state.pr_url.is_none() && state.merged_commit.is_none() {
        state.auto_merge = auto_merge;
    }
    if state.status == "Done" {
        return Ok(state);
    }
    save(&dir, &state)?;
    let result = execute(planning_root, repo, &dir, &mut state, harness, &runner);
    if let Err(error) = result {
        state.status = if runner.cancel.load(Ordering::SeqCst) {
            "Interrupted"
        } else {
            "Needs attention"
        }
        .into();
        state.detail = error.to_string();
        let _ = save(&dir, &state);
        return Err(error);
    }
    save(&dir, &state)?;
    Ok(state)
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
        // Keep the last failure durable so manual resume has the same feedback.
        let mut feedback = state.detail.clone();
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
            let mut prompt = format!(
                "Implement this ticket in the CURRENT working directory, a dedicated Git worktree. This may be a RESUME: inspect git status, existing diffs, commits, untracked files, tests and repository instructions FIRST. Preserve and complete existing work; do not restart, reset, clean, discard or overwrite unrelated changes. Verify prerequisites and dependencies; report blocked if unavailable. Implement only this ticket's scope. Run the required checks and repair failures. Do not change branches, create worktrees, commit, push, create PRs or merge; Packet owns those steps. Do not modify the original checkout.\n\nTICKET PATH: {}\nTICKET CONTENT:\n{}\n\nAPPROVED SPECIFICATION:\n{}\n\nCURRENT STATUS:\n{}\nRECENT COMMITS:\n{}\n\nReturn a complete JSON object with status (complete or blocked), summary, acceptance_criteria (array of objects with criterion copied verbatim from the ticket and concrete evidence), verification (array of runnable POSIX /bin/sh commands; each runs in a NEW shell starting in this worktree, with PACKET_WORKTREE set to its absolute path; no shell variables or cwd changes carry between commands), remaining (array of unresolved work). Complete requires every ticket criterion met, meaningful checks passing, and remaining empty. Use actual commands without placeholder paths. Before changing directories, capture paths or use \"$PACKET_WORKTREE/Cargo.toml\"; $(pwd) after cd refers to the NEW directory. Do not use Bash-only syntax. When testing commands yourself, export PACKET_WORKTREE to this worktree path before invoking /bin/sh. Execute exactly the commands you report using /bin/sh. Assert expected outcomes and preserve command exit failures: capture output to a file, then check it, rather than masking a failed command with a successful pipeline or command substitution. Never claim success from an exit code alone or invent results. Do not include prose outside the JSON.",
                state.ticket, state.ticket_text, specification, status, log
            );

            if !feedback.is_empty() {
                prompt.push_str(&format!("\n\nPREVIOUS STOP / CORRECTION REQUIRED:\n{feedback}\nContinue in this same worktree. Treat this as a correction history: keep earlier fixes and address the newest failure without reintroducing older ones. Inspect and preserve existing work. Correct the report or implementation and rerun affected checks. Copy acceptance criterion text EXACTLY, including any spelling mistakes; do not edit the ticket to satisfy this check. Return the full JSON report, not just the correction. Do not weaken or bypass failing checks. Report blocked for prerequisites that require human intervention.\nPrevious response (possibly truncated):\n{previous_response}"));
            }
            let stamp = chrono::Utc::now().timestamp_nanos_opt().unwrap_or_default();
            let report_path = dir.join(format!("{stamp}-report.json"));
            prompt.push_str(&format!("\n\nRECOVERY REPORT FILE: {}\nAfter verification, atomically write the same complete JSON report to this absolute file (temporary sibling then rename) before your final response. This preserves completion if the CLI loses its final message.\nYou may fix the root cause of encountered failures and add regression coverage in this worktree when necessary. Keep repairs focused, preserve checks, and do not commit them yourself: Packet verifies and commits the task and its recovery fixes together atomically.\n", report_path.display()));
            let request = PlanningRequest { implementation: true, read_only: false, repo_root: state.worktree.clone(), prompt_body: prompt, system_instructions: "You are an implementation agent. Read and follow repository AGENTS.md instructions. Implement, integrate, and verify the whole ticket. Preserve existing work when resuming or correcting a failed report. Return the required JSON report. Report blockers honestly. The application alone manages Git commits, integration, and publication.".into(), timeout: runner.remaining()?, progress_tx: runner.progress.clone(), cancel: runner.cancel.clone() };
            let outcome = match harness.execute(&request) {
                Ok(outcome) => outcome,
                Err(error) => {
                    runner.remaining()?;
                    let detail = error.detail();
                    fs::write(dir.join(format!("{stamp}-harness-error.txt")), &detail)?;
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
                            "Harness recovery exhausted; no working agent response is available to repair further. {feedback}"
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
            // An explicit blocker needs outside intervention, not repeated model calls.
            if let Ok(report) = &parsed {
                if report.status == "blocked" {
                    validate_report(report, &state.ticket_text)?;
                }
            }
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
                    "Automatic correction limit and self-repair attempts exhausted for {phase}. Correction history: {feedback}"
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
        anyhow::ensure!(
            !runner
                .git(
                    &state.worktree,
                    &[
                        "diff",
                        "--name-only",
                        &format!("{}...HEAD", state.base_commit)
                    ]
                )?
                .is_empty(),
            "No implementation changes relative to the starting commit; no PR created"
        );
        state.verified_head = Some(runner.git(&state.worktree, &["rev-parse", "HEAD"])?);
        state.detail = report.summary.clone();
        fs::write(
            dir.join("verified-report.json"),
            serde_json::to_vec_pretty(&report)?,
        )?;
        let body = pr_body(state, &report);
        fs::write(dir.join("pr-body.md"), body)?;
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
        let integration_dir = dir.join(format!("integration-{remote}"));
        fs::create_dir_all(&integration_dir)?;
        let mut integration: Implementation = if integration_dir.join("state.json").exists() {
            serde_json::from_slice(&fs::read(integration_dir.join("state.json"))?)?
        } else {
            let mut record = state.clone();
            record.branch = format!(
                "packet/integration/{}/{}",
                key(&state.ticket),
                &remote[..12]
            );
            record.worktree = state.worktree.with_file_name(format!(
                "{}-integration-{}",
                key(&state.ticket),
                &remote[..12]
            ));
            record.base_commit = remote.clone();
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
                    &["worktree", "add", "-b", &integration.branch, path, &remote],
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
        "Implementation is not complete: {}. Remaining: {}",
        report.summary,
        report.remaining.join("; ")
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
            } else {
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
            let status = if self.mode == "blocked" {
                "blocked"
            } else {
                "complete"
            };
            let verification = if self.mode == "fail"
                || self.mode == "repair_verification"
                || self.mode == "repair_mixed"
            {
                "test -f missing-file"
            } else {
                "test \"$(cat implemented.txt)\" = implemented"
            };
            let mut report = serde_json::json!({"status":status,"summary":"Implemented the ticket behavior.","acceptance_criteria":[{"criterion":"File contains implemented.","evidence":"Created the file and checked its exact contents."}],"verification":[verification],"remaining":[]});
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
                "{}",
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
                assert!(o.status.success(), "{}", String::from_utf8_lossy(&o.stderr));
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
        refresh_pr(&s.repo, &s.ticket, &runner).unwrap();
        assert_eq!(fs::read(dir.join("state.json")).unwrap(), before);
    }
    #[test]
    fn blocked_or_failed_verification_never_creates_pr() {
        for mode in ["blocked", "fail"] {
            let s = Sandbox::new();
            assert!(s.run(mode, Arc::new(AtomicUsize::new(0))).is_err());
            assert!(!s.root.join("pr-created").exists());
            assert!(
                load(&s.repo, &s.ticket)
                    .unwrap()
                    .worktree
                    .join("implemented.txt")
                    .exists()
            );
        }
    }
    #[test]
    fn automatic_corrections_preserve_work_and_publish_only_after_verification() {
        for mode in [
            "repair_markdown",
            "repair_schema",
            "repair_criterion",
            "repair_verification",
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
            ("blocked", 1, "Implementation is not complete"),
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
    fs::write(&temp, serde_json::to_vec(activity)?)?;
    fs::rename(temp, dir.join("activity.json"))?;
    Ok(())
}
