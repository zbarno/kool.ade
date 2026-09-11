//! Resumable ticket implementation. Git worktrees and runtime records are kept
//! independently from planning state; only verified results proceed to a PR.
use crate::{
    error::AppError,
    harness::{AiHarness, LiveProgress, PiHarness, PlanningRequest},
};
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
    pub branch: String,
    pub base: String,
    pub base_commit: String,
    pub worktree: PathBuf,
    pub status: String,
    pub detail: String,
    pub pr_url: Option<String>,
    pub verified_head: Option<String>,
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
    pub fn start(repo: PathBuf, ticket: String) -> Self {
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
            let result = run(&repo, &ticket, &PiHarness, worker_cancel, progress);
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
                    anyhow::ensure!(
                        ok,
                        "{program} failed: {}",
                        if error.is_empty() { &output } else { &error }
                    );
                    return Ok(output.trim().into());
                }
                Err(PollState::Closed) => anyhow::bail!("{program} closed without an exit result"),
                Err(PollState::Pending) => {}
            }
        }
    }
    fn git(&self, cwd: &Path, args: &[&str]) -> anyhow::Result<String> {
        self.command(cwd, "git", args)
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
    let Some(url) = state.pr_url.clone() else {
        return Ok(());
    };
    if state.pr_state.as_deref() == Some("MERGED") {
        return Ok(());
    }
    state.pr_check_attempted_at = Some(chrono::Utc::now().to_rfc3339());
    let result = (|| -> anyhow::Result<String> {
        let output = runner.command(repo, &runner.gh, &["pr", "view", &url, "--json", "state"])?;
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
        "Preparing" | "Implementing" | "Verifying" | "Ready for PR" if busy => 1,
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

pub fn run(
    repo: &Path,
    ticket: &str,
    harness: &dyn AiHarness,
    cancel: Arc<AtomicBool>,
    progress: Sender<LiveProgress>,
) -> anyhow::Result<Implementation> {
    run_with_gh(repo, ticket, harness, cancel, progress, "gh")
}
fn run_with_gh(
    repo: &Path,
    ticket: &str,
    harness: &dyn AiHarness,
    cancel: Arc<AtomicBool>,
    progress: Sender<LiveProgress>,
    gh: &str,
) -> anyhow::Result<Implementation> {
    let runner = Runner {
        gh: gh.into(),
        deadline: Instant::now() + crate::core::turn::configured_turn_timeout(),
        cancel,
        progress,
    };
    let text = read_ticket(repo, ticket)?;
    let dir = state_dir(repo, ticket)?;
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
        let base = runner.git(repo, &["symbolic-ref", "--short", "HEAD"])?;
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
            branch: format!("packet/{}", key(ticket)),
            base,
            base_commit: head,
            worktree: root.join(key(ticket)),
            status: "Preparing".into(),
            detail: String::new(),
            pr_url: None,
            verified_head: None,
            pr_state: None,
            pr_checked_at: None,
            pr_check_attempted_at: None,
            pr_check_error: None,
        }
    };
    save(&dir, &state)?;
    let result = execute(repo, &dir, &mut state, harness, &runner);
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

fn execute(
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
    let already_verified = clean && state.verified_head.as_deref() == Some(head.as_str());
    if already_verified && state.pr_url.is_some() {
        return Ok(());
    }
    if !already_verified {
        state.status = "Implementing".into();
        state.detail.clear();
        save(dir, state)?;
        runner.update(format!(
            "Reviewing and implementing {} in {}",
            state.ticket,
            state.worktree.display()
        ));
        let specification = Path::new(&state.ticket)
            .parent()
            .map(|p| repo.join(p).join("specification.md"))
            .and_then(|p| fs::read_to_string(p).ok())
            .unwrap_or_default();
        let status = runner.git(&state.worktree, &["status", "--short"])?;
        let log = runner.git(&state.worktree, &["log", "-5", "--oneline"])?;
        let prompt = format!(
            "Implement this ticket in the CURRENT working directory, a dedicated Git worktree. This may be a RESUME: inspect git status, existing diffs, commits, untracked files, tests and repository instructions FIRST. Preserve and complete existing work; do not restart, reset, clean, discard or overwrite unrelated changes. Verify prerequisites and dependencies; report blocked if unavailable. Implement only this ticket's scope. Run the required checks and repair failures. Do not change branches, create worktrees, commit, push, create PRs or merge; Packet owns those steps. Do not modify the original checkout.\n\nTICKET PATH: {}\nTICKET CONTENT:\n{}\n\nAPPROVED SPECIFICATION:\n{}\n\nCURRENT STATUS:\n{}\nRECENT COMMITS:\n{}\n\nReturn a complete JSON object with status (complete or blocked), summary, acceptance_criteria (array of objects with criterion copied verbatim from the ticket and concrete evidence), verification (array of runnable shell commands relative to this worktree; Packet will rerun them), remaining (array of unresolved work). Complete requires every ticket criterion met, meaningful checks passing, and remaining empty. Never claim success from an exit code alone or invent results. Do not include prose outside the JSON.",
            state.ticket, state.ticket_text, specification, status, log
        );
        let request = PlanningRequest { implementation: true, repo_root: state.worktree.clone(), prompt_body: prompt, system_instructions: "You are an implementation agent. Read and follow repository AGENTS.md instructions. Understand the ticket-specific problem and goal. Implement, integrate, and verify the whole ticket in the provided worktree. Preserve existing work when resuming. Report blockers honestly. The application alone manages Git commits and PR creation.".into(), timeout: runner.remaining()?, progress_tx: runner.progress.clone(), cancel: runner.cancel.clone() };
        let outcome = harness
            .execute(&request)
            .map_err(|e: AppError| anyhow::anyhow!(e.detail()))?;
        let stamp = chrono::Utc::now().timestamp_nanos_opt().unwrap_or_default();
        fs::write(
            dir.join(format!("{stamp}-response.txt")),
            &outcome.final_text,
        )?;
        runner.remaining()?;
        let json = crate::harness::pi_extract::extract_json_object(&outcome.final_text)
            .ok_or_else(|| {
                anyhow::anyhow!(
                    "Pi returned no complete implementation report. Worktree preserved for resume."
                )
            })?;
        let report: Report = serde_json::from_str(&json)?;
        validate_report(&report, &state.ticket_text)?;
        state.status = "Verifying".into();
        save(dir, state)?;
        let mut evidence = Vec::new();
        for command in &report.verification {
            runner.update(format!("Verifying: {command}"));
            let result = runner.command(&state.worktree, "/bin/sh", &["-c", command]);
            evidence.push(serde_json::json!({"command":command,"output":result.as_ref().ok(),"error":result.as_ref().err().map(ToString::to_string)}));
            fs::write(
                dir.join(format!("{stamp}-verification.json")),
                serde_json::to_vec_pretty(&evidence)?,
            )?;
            result?;
        }
        anyhow::ensure!(
            runner.git(&state.worktree, &["symbolic-ref", "--short", "HEAD"])? == state.branch,
            "Implementation changed branches; refusing to publish"
        );
        runner.git(&state.worktree, &["diff", "--check"])?;
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
        let body = pr_body(state, &report);
        fs::write(dir.join("pr-body.md"), body)?;
        state.status = "Ready for PR".into();
        save(dir, state)?;
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
            self.calls.fetch_add(1, Ordering::SeqCst);
            assert!(req.implementation);
            assert!(req.prompt_body.contains("RESUME"));
            if self.mode == "resume" {
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
            let status = if self.mode == "blocked" {
                "blocked"
            } else {
                "complete"
            };
            let verification = if self.mode == "fail" {
                "test -f missing-file"
            } else {
                "test \"$(cat implemented.txt)\" = implemented"
            };
            Ok(crate::harness::HarnessOutcome { final_text: serde_json::json!({"status":status,"summary":"Implemented the ticket behavior.","acceptance_criteria":[{"criterion":"File contains implemented.","evidence":"Created the file and checked its exact contents."}],"verification":[verification],"remaining":[]}).to_string(), envelope:None, stderr_tail:String::new() })
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
