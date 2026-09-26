//! Resumable ticket implementation. Git worktrees and runtime records are kept
//! independently from planning state; only verified results proceed to a PR.
mod activity;
mod board_states;
mod checks;
mod checks_gate;
pub mod cleanup;
mod integration;
mod lifecycle;
mod pr_refresh;
mod publication;
mod recovery;
pub(crate) mod report;
mod runner;
mod state;
mod state_paths;
pub mod status;
mod task;
mod verification;
#[cfg(test)]
use activity::{
    ACTIVITY_MAX_FIELD_CHARS, ACTIVITY_MAX_POST_CHARS, ACTIVITY_MAX_POSTS, retain_suffix,
    trim_for_persist,
};
pub use activity::{load_activity, save_activity};
pub use board_states::{BOARD_COLUMNS, board_column, load_board_states};
pub use pr_refresh::PrRefresh;
#[cfg(test)]
use pr_refresh::refresh_pr;
use publication::pr_body;
#[cfg(test)]
use publication::remote_repository;
pub use recovery::latest_external_blocker;
pub(crate) use report::{BlockerDisposition, Report, ReportStatus, parse_report};
use report::{external_blocker, external_blocker_detail, validate_report};
use runner::{Runner, append_tail};
use state::save;
pub(crate) use state::{decode_state_bytes, read_state_file, serialize_state};
use state_paths::{common, key, state_dir_for_task};
pub(crate) use state_paths::{key_for_ticket, state_dir};
pub(crate) use task::permits_evidence_only_completion;
#[cfg(test)]
use task::read_ticket;
#[cfg(test)]
use task::task_repository_id;
pub use task::{completed_dependency_context, target_repository};
use task::{
    read_ticket_and_identity, scoped_product_context, specification_matches_task, ticket_identity,
    title,
};
#[cfg(test)]
mod identity_tests;
#[cfg(test)]
#[path = "implementation/metadata_tests.rs"]
mod metadata_tests;
pub use status::{
    Failure, FailureKind, ImplementationStatus, IndependentCheck, IndependentCheckStatus,
    PublicationStatus, PullRequestState, RecoveryDisposition,
};

use crate::harness::{AiHarness, LiveProgress, PlanningRequest};
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
    /// Stable task identity; `ticket` remains the current path hint.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub task_uid: Option<String>,
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
    pub status: ImplementationStatus,
    pub detail: String,
    pub pr_url: Option<String>,
    pub verified_head: Option<String>,
    #[serde(default)]
    pub auto_merge: bool,
    #[serde(default)]
    pub merged_commit: Option<String>,
    #[serde(default)]
    pub pr_state: Option<PullRequestState>,
    #[serde(default)]
    pub pr_checked_at: Option<String>,
    #[serde(default)]
    pub pr_check_attempted_at: Option<String>,
    #[serde(default)]
    pub pr_check_error: Option<String>,
    #[serde(default)]
    pub independent_check: Option<IndependentCheck>,
    #[serde(default)]
    pub cleanup: cleanup::Cleanup,
}
pub enum Event {
    Progress(LiveProgress),
    Done(Box<Result<Implementation, Failure>>),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum PublicationMode {
    HoldForReview,
    CreatePullRequest,
    AutoPublish,
}

struct RunOptions<'a> {
    harness: &'a dyn AiHarness,
    cancel: Arc<AtomicBool>,
    progress: Sender<LiveProgress>,
    gh: &'a str,
    publication_mode: PublicationMode,
    require_independent_checks: bool,
    user_context: Option<&'a str>,
    auto_publish_gate: Option<Arc<AtomicBool>>,
}

struct ExecutionPolicy<'a> {
    user_context: Option<&'a str>,
    publication_mode: PublicationMode,
    require_independent_checks: bool,
    auto_publish_gate: Option<&'a AtomicBool>,
}

pub struct Controller {
    #[cfg(test)]
    _keep_alive: Option<Sender<Event>>,
    rx: Receiver<Event>,
    cancel: Arc<AtomicBool>,
    auto_publish_gate: Arc<AtomicBool>,
}
impl Controller {
    #[cfg(test)]
    pub(crate) fn idle_fixture() -> Self {
        let (tx, rx) = mpsc::channel();
        Self {
            _keep_alive: Some(tx),
            rx,
            cancel: Arc::new(AtomicBool::new(false)),
            auto_publish_gate: Arc::new(AtomicBool::new(false)),
        }
    }
    #[cfg(test)]
    pub(crate) fn cancellation_requested(&self) -> bool {
        self.cancel.load(Ordering::SeqCst)
    }
    pub fn start(
        repo: PathBuf,
        ticket: String,
        auto_merge: bool,
        harness: Box<dyn AiHarness>,
    ) -> Self {
        Self::start_project(repo.clone(), repo, ticket, auto_merge, harness)
    }
    pub fn start_project(
        planning_root: PathBuf,
        target_repo: PathBuf,
        ticket: String,
        auto_merge: bool,
        harness: Box<dyn AiHarness>,
    ) -> Self {
        Self::start_project_with_context(
            planning_root,
            target_repo,
            ticket,
            auto_merge,
            None,
            harness,
        )
    }
    pub fn start_project_with_context(
        planning_root: PathBuf,
        target_repo: PathBuf,
        ticket: String,
        auto_merge: bool,
        user_context: Option<String>,
        harness: Box<dyn AiHarness>,
    ) -> Self {
        let mode = if auto_merge {
            PublicationMode::AutoPublish
        } else {
            PublicationMode::HoldForReview
        };
        Self::start_project_with_policy(
            planning_root,
            target_repo,
            ticket,
            mode,
            false,
            user_context,
            harness,
        )
    }

    pub(crate) fn start_project_with_policy(
        planning_root: PathBuf,
        target_repo: PathBuf,
        ticket: String,
        publication_mode: PublicationMode,
        require_independent_checks: bool,
        user_context: Option<String>,
        harness: Box<dyn AiHarness>,
    ) -> Self {
        let (tx, rx) = mpsc::channel();
        let cancel = Arc::new(AtomicBool::new(false));
        let worker_cancel = cancel.clone();
        let auto_publish_gate = Arc::new(AtomicBool::new(
            publication_mode == PublicationMode::AutoPublish,
        ));
        let worker_publish_gate = auto_publish_gate.clone();
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
                RunOptions {
                    harness: harness.as_ref(),
                    cancel: worker_cancel,
                    progress,
                    gh: "gh",
                    publication_mode,
                    require_independent_checks,
                    user_context: user_context.as_deref(),
                    auto_publish_gate: Some(worker_publish_gate),
                },
            );
            let _ = forward.join();
            let _ = tx.send(Event::Done(Box::new(
                result.map_err(|error| Failure::from_error(&error)),
            )));
        });
        Self {
            rx,
            cancel,
            auto_publish_gate,
            #[cfg(test)]
            _keep_alive: None,
        }
    }
    pub fn poll(&self) -> Option<Event> {
        match self.rx.try_recv() {
            Ok(event) => Some(event),
            Err(mpsc::TryRecvError::Empty) => None,
            Err(mpsc::TryRecvError::Disconnected) => {
                Some(Event::Done(Box::new(Err(Failure::new(
                    FailureKind::Other,
                    RecoveryDisposition::ExplicitResume,
                    "Implementation worker stopped without a result. Work is preserved; inspect the task failure and Resume implementation.",
                )))))
            }
        }
    }
    pub fn request_cancel(&self) {
        self.cancel.store(true, Ordering::SeqCst);
    }

    pub fn disable_automatic_publication(&self) {
        self.auto_publish_gate.store(false, Ordering::SeqCst);
    }
}
impl Drop for Controller {
    fn drop(&mut self) {
        self.request_cancel();
    }
}

pub fn load_all(repo: &Path) -> Vec<Implementation> {
    let Ok(entries) =
        fs::read_dir(crate::artifacts::layout::ArtifactLayout::new(repo).implementation_root())
    else {
        return Vec::new();
    };
    entries
        .filter_map(Result::ok)
        .filter_map(|entry| read_state_file(&entry.path().join("state.json")).ok())
        .collect()
}
pub fn load(repo: &Path, ticket: &str) -> Option<Implementation> {
    let uid = ticket_identity(repo, ticket).ok().flatten();
    let directory = state_dir_for_task(repo, ticket, uid.as_deref()).ok()?;
    read_state_file(&directory.join("state.json")).ok()
}

fn resume_failure_context(detail: &str) -> String {
    // Also unwrap legacy errors whose complete correction histories were nested
    // on every resume. Keep only the newest diagnostic in the active prompt.
    let start = detail
        .rfind("\nAttempt ")
        .into_iter()
        .chain(detail.rfind("\nHarness failure "))
        .max();
    let latest = start.map(|index| &detail[index + 1..]).unwrap_or(detail);
    let latest = latest
        .rsplit_once("Latest failure: ")
        .map(|(_, tail)| tail)
        .unwrap_or(latest);
    let latest = latest
        .split("\nAUTOMATIC BLOCKER RECOVERY REQUIRED")
        .next()
        .unwrap_or(latest);
    let latest = latest
        .split("\nSELF-REPAIR REQUIRED")
        .next()
        .unwrap_or(latest);
    crate::core::context_build::clip(latest, 4000)
}

pub fn run(
    repo: &Path,
    ticket: &str,
    harness: &dyn AiHarness,
    cancel: Arc<AtomicBool>,
    progress: Sender<LiveProgress>,
) -> anyhow::Result<Implementation> {
    run_with_project_options(
        repo,
        repo,
        ticket,
        RunOptions {
            harness,
            cancel,
            progress,
            gh: "gh",
            publication_mode: PublicationMode::HoldForReview,
            require_independent_checks: false,
            user_context: None,
            auto_publish_gate: None,
        },
    )
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
#[cfg(test)]
fn run_with_options(
    repo: &Path,
    ticket: &str,
    harness: &dyn AiHarness,
    cancel: Arc<AtomicBool>,
    progress: Sender<LiveProgress>,
    gh: &str,
    auto_merge: bool,
) -> anyhow::Result<Implementation> {
    let mode = if auto_merge {
        PublicationMode::AutoPublish
    } else {
        PublicationMode::CreatePullRequest
    };
    run_with_project_options(
        repo,
        repo,
        ticket,
        RunOptions {
            harness,
            cancel,
            progress,
            gh,
            publication_mode: mode,
            require_independent_checks: false,
            user_context: None,
            auto_publish_gate: None,
        },
    )
}
fn run_with_project_options(
    planning_root: &Path,
    repo: &Path,
    ticket: &str,
    options: RunOptions<'_>,
) -> anyhow::Result<Implementation> {
    let RunOptions {
        harness,
        cancel,
        progress,
        gh,
        publication_mode,
        require_independent_checks,
        user_context,
        auto_publish_gate,
    } = options;
    let migration_gate = crate::artifacts::migration::acquire_project_state_gate(planning_root)?;
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
    let (text, task_uid, _) = read_ticket_and_identity(planning_root, ticket)?;
    let dir = state_dir_for_task(planning_root, ticket, task_uid.as_deref())?;
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
        let mut state = read_state_file(&dir.join("state.json"))?;
        anyhow::ensure!(
            (state.ticket == ticket
                || task_uid
                    .as_deref()
                    .is_some_and(|uid| state.task_uid.as_deref() == Some(uid)))
                && state.ticket_text == text
                && state
                    .task_uid
                    .as_deref()
                    .is_none_or(|uid| task_uid.as_deref() == Some(uid)),
            "Ticket changed since implementation started. Review the existing worktree before starting a revised ticket."
        );
        state.ticket = ticket.to_owned();
        if task_uid.is_some() {
            state.task_uid = task_uid.clone();
        }
        save(&dir, &state)?;
        state
    } else {
        // Persist identity before worktree creation so crashes can be resumed.
        let base = if publication_mode == PublicationMode::AutoPublish {
            publication::default_branch(repo, &runner)?
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
        } else if publication_mode == PublicationMode::AutoPublish
            && runner
                .git(repo, &["merge-base", "--is-ancestor", &remote, &local])
                .is_err()
        {
            // Auto workers start from current remote truth, leaving divergent
            // local development history intact in the operator's checkout.
            remote
        } else {
            runner.git(repo, &["merge-base", "--is-ancestor", &remote, &local])
                .map_err(|_| {
                    anyhow::Error::new(crate::core::implementation::status::FailureCause(
                        Failure::new(
                            FailureKind::RemoteDiverged,
                            RecoveryDisposition::UserAction,
                            format!("Local {base} and freshly fetched origin/{base} have diverged. Reconcile the branch before implementing; no work was discarded."),
                        ),
                    ))
                })?;
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
            task_uid: task_uid.clone(),
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
            status: ImplementationStatus::Preparing,
            detail: String::new(),
            pr_url: None,
            verified_head: None,
            auto_merge: publication_mode == PublicationMode::AutoPublish,
            merged_commit: None,
            pr_state: None,
            pr_checked_at: None,
            pr_check_attempted_at: None,
            pr_check_error: None,
            independent_check: None,
            cleanup: Default::default(),
        }
    };
    if state.pr_url.is_none() && state.merged_commit.is_none() {
        state.auto_merge = publication_mode == PublicationMode::AutoPublish;
    }
    if state.status == ImplementationStatus::Completed {
        return Ok(state);
    }
    save(&dir, &state)?;
    drop(migration_gate);
    let result = runner
        .check_storage(&dir)
        .and_then(|_| runner.check_storage(&state.worktree))
        .and_then(|_| {
            lifecycle::execute(
                repo,
                &dir,
                &mut state,
                harness,
                &runner,
                ExecutionPolicy {
                    user_context,
                    publication_mode,
                    require_independent_checks,
                    auto_publish_gate: auto_publish_gate.as_deref(),
                },
            )
        });
    if let Err(error) = result {
        state.status = if runner.cancel.load(Ordering::SeqCst) {
            ImplementationStatus::Interrupted
        } else {
            ImplementationStatus::Blocked
        };
        state.detail = format!("{error:#}");
        if let Err(save_error) = save(&dir, &state) {
            anyhow::bail!(
                "{}\nCould not persist the failed task state at {}: {save_error:#}. Check available disk space and permissions, then Resume implementation.",
                state.detail,
                dir.display()
            );
        }
        return Err(error);
    }
    // Completion is already durable. Reclamation has its own bounded budget
    // and records failure without turning a published task back into a failure.
    let cleanup_runner = Runner {
        gh: runner.gh.clone(),
        deadline: Instant::now() + Duration::from_secs(120),
        cancel: runner.cancel.clone(),
        progress: runner.progress.clone(),
    };
    cleanup::run(repo, &dir, &mut state, &cleanup_runner);
    if let Err(error) = save(&dir, &state) {
        if state.status != ImplementationStatus::Completed {
            return Err(error);
        }
        state.cleanup.error = Some(format!(
            "Could not save cleanup outcome: {error:#}. {}",
            state
                .cleanup
                .error
                .as_deref()
                .unwrap_or("Cleanup will be checked again on the next refresh.")
        ));
    }
    Ok(state)
}

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
            evidence.push_str(&format!(
                "\n{anchor}: not a resolvable commit in this checkout\n"
            ));
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
        assert!(
            matches!(controller.poll(), Some(Event::Done(result)) if matches!(&*result, Err(message) if message.message.contains("stopped without a result")))
        );
    }

    #[test]
    fn completed_worker_delivers_result_before_disconnect() {
        let controller = Controller::idle_fixture();
        controller
            ._keep_alive
            .as_ref()
            .unwrap()
            .send(Event::Done(Box::new(Err(Failure::other("original cause")))))
            .unwrap();
        assert!(
            matches!(controller.poll(), Some(Event::Done(result)) if matches!(&*result, Err(message) if message.message == "original cause"))
        );
    }

    #[test]
    fn implementation_context_uses_only_frozen_affected_product_modules() {
        let root = std::env::temp_dir().join(format!(
            "packet_implementation_context_{}-{}",
            std::process::id(),
            chrono::Utc::now().timestamp_nanos_opt().unwrap()
        ));
        let batch = root.join(".kool-ade-packet/planning/tasks/feature");
        fs::create_dir_all(&batch).unwrap();
        let contract = crate::core::contract_snapshot::BatchContract {
            feature_id: "CHG-001".into(),
            feature_specification: "# Feature".into(),
            product_modules: [("current-capabilities".into(), "## 5. Relevant\n".into())].into(),
            repository_bases: Default::default(),
            configuration: String::new(),
        };
        fs::write(
            batch.join("contract.json"),
            serde_json::to_vec(&contract).unwrap(),
        )
        .unwrap();
        let context =
            scoped_product_context(&root, ".kool-ade-packet/planning/tasks/feature/001-task.md")
                .unwrap()
                .unwrap();
        assert!(context.contains("product:current-capabilities"));
        assert!(context.contains("## 5. Relevant"));
        assert!(!context.contains("product:overview"));
        let _ = fs::remove_dir_all(root);
    }
    #[test]
    fn cross_repository_dependency_context_requires_merged_record() {
        let root = std::env::temp_dir().join(format!(
            "packet_dependency_context_{}-{}",
            std::process::id(),
            chrono::Utc::now().timestamp_nanos_opt().unwrap()
        ));
        let batch = root.join(".kool-ade-packet/planning/tasks/feature");
        fs::create_dir_all(&batch).unwrap();
        assert!(
            std::process::Command::new("git")
                .args(["init", "-q"])
                .current_dir(&root)
                .status()
                .unwrap()
                .success()
        );
        let prior = ".kool-ade-packet/planning/tasks/feature/001-api.md";
        let prior_text =
            "# API contract\n\nRepository: api\n\nThe endpoint returns a saved search.\n";
        fs::write(root.join(prior), prior_text).unwrap();
        let next = ".kool-ade-packet/planning/tasks/feature/002-web.md";
        let next_text =
            "# Web client\n\nRepository: web\n\n## Dependencies\n\n- [API contract](001-api.md)\n";
        fs::write(root.join(next), next_text).unwrap();
        assert!(completed_dependency_context(&root, next, next_text).is_err());
        let record: Implementation = serde_json::from_value(serde_json::json!({
            "ticket": prior, "ticket_text": prior_text, "branch": "packet/api", "base": "main",
            "base_commit": "base", "worktree": root, "status": "completed", "detail": "",
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
            assert_eq!(req.mode, crate::harness::ExecutionMode::Implementation);
            assert!(req.prompt_body.contains("RESUME"));
            if self.mode == "fresh_budget" {
                assert!(
                    req.prompt_body
                        .contains("fresh report, verification, harness, and self-repair budget")
                );
                assert!(
                    !req.prompt_body
                        .contains("Automatic correction limit and self-repair attempts exhausted")
                );
                assert!(
                    !req.prompt_body
                        .contains("Correction history: Correction history:")
                );
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
            let status = if matches!(self.mode, "blocked" | "external_blocked")
                || (self.mode == "repair_blocked" && call == 0)
            {
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
            let blocker_disposition = if status == "blocked" {
                if self.mode == "external_blocked" {
                    "human_action"
                } else {
                    "machine_repair"
                }
            } else {
                "none"
            };
            let mut report = serde_json::json!({"schemaVersion":1,"status":status,"blocker_disposition":blocker_disposition,"summary":"Implemented the ticket behavior.","acceptance_criteria":[{"criterion":criterion,"evidence":"Created the required evidence and checked its exact contents."}],"verification":[verification],"remaining":[]});
            if status == "blocked" {
                report["remaining"] = if self.mode == "external_blocked" {
                    serde_json::json!([
                        "Adjudicator: approve the revised contract before resuming."
                    ])
                } else {
                    serde_json::json!([
                        "Repair the local conductor and rerun the acceptance check."
                    ])
                };
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
            let ticket = ".kool-ade-packet/planning/tasks/feature/001-implement-ticket-behavior.md"
                .to_owned();
            fs::create_dir_all(repo.join(".kool-ade-packet/planning/tasks/feature")).unwrap();
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
        fn run_with_publication_policy(
            &self,
            fixture_mode: &'static str,
            calls: Arc<AtomicUsize>,
            publication_mode: PublicationMode,
            auto_publish_gate: Option<Arc<AtomicBool>>,
            require_independent_checks: bool,
        ) -> anyhow::Result<Implementation> {
            let (tx, _rx) = mpsc::channel();
            run_with_project_options(
                &self.repo,
                &self.repo,
                &self.ticket,
                RunOptions {
                    harness: &Fixture {
                        mode: fixture_mode,
                        calls,
                    },
                    cancel: Arc::new(AtomicBool::new(false)),
                    progress: tx,
                    gh: self.gh.to_str().unwrap(),
                    publication_mode,
                    require_independent_checks,
                    user_context: None,
                    auto_publish_gate,
                },
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

    struct ImmediateCheck(bool);
    impl checks::Provider for ImmediateCheck {
        fn name(&self) -> &'static str {
            "Fixture checks"
        }

        fn repository(&self, _remote: &str) -> Option<String> {
            Some("github.com/fixture/repo".into())
        }

        fn check(
            &self,
            runner: &Runner,
            cwd: &Path,
            repository: &str,
            commit: &str,
        ) -> anyhow::Result<checks::ResultState> {
            assert_eq!(repository, "github.com/fixture/repo");
            let reference = checks::candidate_ref("fixture", commit);
            let remote = runner.git(cwd, &["ls-remote", "origin", &reference])?;
            assert!(
                remote.contains(commit),
                "candidate was not pushed before checking"
            );
            Ok(if self.0 {
                checks::ResultState::Passed
            } else {
                checks::ResultState::Failed("fixture workflow failed".into())
            })
        }
    }
    #[test]
    fn isolated_implementation_verifies_pushes_and_reuses_pr() {
        let s = Sandbox::new();
        fs::write(s.repo.join("unrelated.txt"), "main checkout draft").unwrap();
        let calls = Arc::new(AtomicUsize::new(0));
        let result = s.run("complete", calls.clone()).unwrap();
        assert_eq!(result.status, ImplementationStatus::AwaitingReview);
        assert!(result.pr_url.is_some());
        assert!(
            !crate::artifacts::layout::ArtifactLayout::new(&s.repo)
                .decisions_root()
                .exists(),
            "implementing a task alone must not create an ADR"
        );
        assert!(
            state_dir(&s.repo, &s.ticket)
                .unwrap()
                .join("verified-report.json")
                .exists(),
            "verification evidence stays in the implementation record"
        );
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
    fn disabled_auto_publish_keeps_verified_work_local_until_explicit_pr_action() {
        let s = Sandbox::new();
        let remote_main = s.git(&s.repo, &["rev-parse", "origin/main"]);
        let calls = Arc::new(AtomicUsize::new(0));

        let held = s
            .run_with_publication_policy(
                "complete",
                calls.clone(),
                PublicationMode::AutoPublish,
                Some(Arc::new(AtomicBool::new(false))),
                false,
            )
            .unwrap();
        assert_eq!(held.status, ImplementationStatus::ReadyToPublish);
        assert!(held.pr_url.is_none() && held.merged_commit.is_none());
        assert!(held.worktree.exists());
        assert!(
            state_dir(&s.repo, &s.ticket)
                .unwrap()
                .join("verified-report.json")
                .exists()
        );
        assert!(!s.root.join("pr-created").exists());
        assert_eq!(s.git(&s.repo, &["rev-parse", "origin/main"]), remote_main);

        let pr = s
            .run_with_publication_policy(
                "complete",
                calls.clone(),
                PublicationMode::CreatePullRequest,
                None,
                false,
            )
            .unwrap();
        assert_eq!(pr.status, ImplementationStatus::AwaitingReview);
        assert_eq!(
            pr.pr_url.as_deref(),
            Some("https://github.com/fixture/repo/pull/1")
        );
        assert_eq!(
            calls.load(Ordering::SeqCst),
            1,
            "verified implementation is reused"
        );
        assert!(s.root.join("pr-created").exists());
    }

    #[test]
    fn required_independent_checks_fail_closed_for_unsupported_git_hosting() {
        let s = Sandbox::new();
        let remote_main = s.git(&s.repo, &["rev-parse", "origin/main"]);
        let calls = Arc::new(AtomicUsize::new(0));
        let error = s
            .run_with_publication_policy(
                "complete",
                calls,
                PublicationMode::AutoPublish,
                None,
                true,
            )
            .unwrap_err();
        assert!(error.to_string().contains("no supported CI provider"));
        let classified = Failure::from_error(&error);
        assert_eq!(classified.kind, FailureKind::ExternalPrerequisite);
        assert_eq!(classified.recovery, RecoveryDisposition::UserAction);
        assert_eq!(s.git(&s.repo, &["rev-parse", "origin/main"]), remote_main);
        let state_dir = state_dir(&s.repo, &s.ticket).unwrap();
        let state = read_state_file(&state_dir.join("state.json")).unwrap();
        assert_eq!(state.status, ImplementationStatus::Blocked);
        let check = state.independent_check.unwrap();
        assert_eq!(check.status, IndependentCheckStatus::Unavailable);
        assert!(check.detail.unwrap().contains("does not support"));
        assert!(state_dir.join("verified-report.json").exists());
    }

    #[test]
    fn independent_check_gate_pushes_only_the_candidate_and_records_the_exact_result() {
        for passes in [true, false] {
            let s = Sandbox::new();
            let remote_main = s.git(&s.repo, &["rev-parse", "origin/main"]);
            let calls = Arc::new(AtomicUsize::new(0));
            let mut state = s.run("complete", calls).unwrap();
            assert!(state.worktree.exists());
            let commit = state.verified_head.clone().unwrap();
            let candidate_ref = checks::candidate_ref("fixture", &commit);
            let worktree = state.worktree.clone();
            let dir = state_dir(&s.repo, &s.ticket).unwrap();
            let (progress, _updates) = mpsc::channel();
            let runner = Runner {
                gh: s.gh.to_string_lossy().into_owned(),
                deadline: Instant::now() + Duration::from_secs(60),
                cancel: Arc::new(AtomicBool::new(false)),
                progress,
            };
            let outcome = checks_gate::wait_with_provider(
                checks_gate::CheckRequest {
                    provider: &ImmediateCheck(passes),
                    repository: "github.com/fixture/repo",
                    candidate_ref: &candidate_ref,
                    worktree: &worktree,
                    commit: &commit,
                },
                &dir,
                &mut state,
                &runner,
                None,
            );
            if passes {
                outcome.unwrap();
                assert_eq!(state.status, ImplementationStatus::Publishing);
                assert_eq!(
                    state.independent_check.as_ref().unwrap().status,
                    IndependentCheckStatus::Passed
                );
            } else {
                let error = outcome.unwrap_err();
                assert!(error.to_string().contains("Project checks failed"));
                let classified = Failure::from_error(&error);
                assert_eq!(classified.kind, FailureKind::Verification);
                assert_eq!(classified.recovery, RecoveryDisposition::ExplicitResume);
                assert_eq!(
                    state.independent_check.as_ref().unwrap().status,
                    IndependentCheckStatus::Failed
                );
            }
            assert_eq!(s.git(&s.repo, &["rev-parse", "origin/main"]), remote_main);
            assert!(
                s.git(&s.repo, &["ls-remote", "origin", &candidate_ref])
                    .contains(&commit),
                "the exact integration commit should be on the temporary checks ref"
            );
        }
    }
    #[test]
    fn explicit_evidence_only_task_completes_without_commit_or_pr() {
        let s = Sandbox::new();
        s.make_evidence_only();
        let base = s.git(&s.repo, &["rev-parse", "HEAD"]);
        let calls = Arc::new(AtomicUsize::new(0));

        let result = s.run("evidence_only", calls.clone()).unwrap();

        assert_eq!(calls.load(Ordering::SeqCst), 1);
        assert_eq!(result.status, ImplementationStatus::Completed);
        assert_eq!(result.verified_head.as_deref(), Some(base.as_str()));
        assert_eq!(result.merged_commit.as_deref(), Some(base.as_str()));
        assert!(result.pr_url.is_none());
        assert!(!s.root.join("pr-created").exists());
        assert!(
            result.cleanup.completed_at.is_some(),
            "{:?}",
            result.cleanup
        );
        assert!(!result.worktree.exists());
        assert_eq!(s.git(&s.repo, &["rev-parse", "HEAD"]), base);
        assert!(
            state_dir(&s.repo, &s.ticket)
                .unwrap()
                .join("verified-report.json")
                .exists()
        );
    }
    #[test]
    fn verification_contract_completes_without_product_changes() {
        let s = Sandbox::new();
        fs::write(s.repo.join(&s.ticket), "# Verify regression\n\nThis ticket is pure verification, commits no bytes.\n\n## Acceptance criteria\n\n- Repository remains unchanged.\n\nThis ticket itself changed no repository file.\n").unwrap();
        let result = s
            .run("evidence_only", Arc::new(AtomicUsize::new(0)))
            .unwrap();
        assert_eq!(result.status, ImplementationStatus::Completed);
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
        assert_eq!(state.status, ImplementationStatus::Interrupted);
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
        state.status = ImplementationStatus::Blocked;
        save(&state_dir(&s.repo, ticket).unwrap(), &state).unwrap();
        let resumed = s.run("resume", calls.clone()).unwrap();
        assert_eq!(resumed.worktree, state.worktree);
        assert_eq!(resumed.base_commit, state.base_commit);
        assert_eq!(resumed.status, ImplementationStatus::AwaitingReview);
        assert_eq!(calls.load(Ordering::SeqCst), 2);
    }

    #[test]
    fn implementation_reads_only_canonical_board_task_paths() {
        let s = Sandbox::new();
        let root = ".kool-ade-packet/planning/tasks";
        fs::create_dir_all(s.repo.join(root).join("validation")).unwrap();
        for name in [
            "001-task.md",
            "CHG-003-TASK-verify.md",
            "F10-TASK-verify.md",
            "README.md",
            "specification.md",
            "001-task.txt",
            "invalid-TASK-verify.md",
        ] {
            let path = format!("{root}/validation/{name}");
            fs::write(s.repo.join(&path), "# Fixture task").unwrap();
            assert_eq!(
                read_ticket(&s.repo, &path).is_ok(),
                crate::artifacts::task_docs::is_task_story_filename(name),
                "{path}"
            );
        }
        fs::create_dir_all(s.repo.join("planning/tasks/validation")).unwrap();
        fs::write(
            s.repo.join("planning/tasks/validation/001-task.md"),
            "# Legacy fixture task",
        )
        .unwrap();
        assert!(read_ticket(&s.repo, "planning/tasks/validation/001-task.md").is_err());
        assert!(read_ticket(&s.repo, ".kool-ade-packet/planning/tasks/../001-task.md").is_err());
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
            assert_eq!(state.pr_state.map(PullRequestState::label), Some(value));
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
        assert_eq!(
            load(&s.repo, &s.ticket).unwrap().status,
            ImplementationStatus::Completed
        );
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
    fn external_decision_stops_after_one_report_and_preserves_work() {
        let s = Sandbox::new();
        let calls = Arc::new(AtomicUsize::new(0));
        let result = s.run("external_blocked", calls.clone());
        assert!(result.is_err());
        assert_eq!(calls.load(Ordering::SeqCst), 1);
        let state = load(&s.repo, &s.ticket).unwrap();
        assert_eq!(state.status, ImplementationStatus::Blocked);
        assert!(
            state
                .detail
                .contains("Adjudicator: approve the revised contract")
        );
        assert!(state.worktree.join("implemented.txt").exists());
        assert!(!s.root.join("pr-created").exists());
    }
    #[test]
    fn feature_id_collision_does_not_inject_another_features_specification() {
        assert!(!specification_matches_task(
            "# Task\n\nFeature: Switch Workspaces\n",
            "# CHG-003: Readable Chat Replies\n"
        ));
        assert!(specification_matches_task(
            "# Task\n\nFeature: Switch Workspaces\n",
            "# CHG-003: Switch Workspaces\n"
        ));
    }
    #[test]
    fn newest_saved_report_controls_interrupted_blocker_recovery() {
        let s = Sandbox::new();
        let dir = state_dir(&s.repo, &s.ticket).unwrap();
        fs::create_dir_all(&dir).unwrap();
        let blocked = serde_json::json!({
            "status":"blocked", "summary":"The history needs a decision.",
            "blocker_disposition":"human_action",
            "acceptance_criteria":[], "verification":[],
            "remaining":["Adjudicator: approve the realized footprint."]
        });
        fs::write(dir.join("001-report.json"), blocked.to_string()).unwrap();
        assert!(
            latest_external_blocker(&s.repo, &s.ticket)
                .unwrap()
                .contains("approve the realized footprint")
        );
        let complete = serde_json::json!({
            "status":"complete", "blocker_disposition":"none", "summary":"Done", "acceptance_criteria":[],
            "verification":[], "remaining":[]
        });
        fs::write(dir.join("002-report.json"), complete.to_string()).unwrap();
        assert!(latest_external_blocker(&s.repo, &s.ticket).is_none());
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
            assert_eq!(result.status, ImplementationStatus::AwaitingReview);
            let dir = state_dir(&s.repo, &s.ticket).unwrap();
            let files = fs::read_dir(&dir)
                .unwrap()
                .map(|entry| entry.unwrap().file_name().to_string_lossy().into_owned())
                .collect::<Vec<_>>();
            let incomplete_response = matches!(mode, "repair_markdown" | "repair_schema");
            assert_eq!(
                files.iter().filter(|name| name.ends_with("-report.json") && *name != "verified-report.json").count(),
                if incomplete_response { 1 } else { 2 }
            );
            assert_eq!(
                files
                    .iter()
                    .filter(|name| name.ends_with("-response.txt"))
                    .count(),
                usize::from(incomplete_response),
                "only incomplete final responses need a separate copy"
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
        assert_eq!(
            calls.load(Ordering::SeqCst),
            12,
            "resume gets all six attempts again"
        );
        assert_eq!(second.matches("Automatic correction limit").count(), 1);
        assert!(!second.contains("Correction history:"));
        assert!(second.contains("Latest failure:"));
        assert!(!s.root.join("pr-created").exists());
        let dir = state_dir(&s.repo, &s.ticket).unwrap();
        assert!(fs::read_dir(&dir).unwrap().flatten().any(|entry| {
            entry
                .file_name()
                .to_string_lossy()
                .ends_with("-resume-context.txt")
                && fs::read_to_string(entry.path()).unwrap() == first
        }));
        let resumed = s
            .run("fresh_budget", Arc::new(AtomicUsize::new(0)))
            .unwrap();
        assert_eq!(resumed.worktree, original.worktree);
        assert_eq!(resumed.base_commit, original.base_commit);
        assert_eq!(resumed.status, ImplementationStatus::AwaitingReview);
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
            ImplementationStatus::AwaitingReview
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
        Runner {
            gh: "unused".into(),
            deadline: Instant::now() + Duration::from_secs(30),
            cancel: Arc::new(AtomicBool::new(false)),
            progress,
        }
    }

    // Produce a completed record without invoking automatic cleanup: models an
    // older Packet version leaving a merged PR's worktree behind.
    fn completed_cleanup_fixture(s: &Sandbox) -> Implementation {
        let mut state = s.run("complete", Arc::new(AtomicUsize::new(0))).unwrap();
        let head = state.verified_head.as_deref().unwrap();
        s.git(
            &s.repo,
            &["push", "origin", &format!("{head}:refs/heads/main")],
        );
        state.status = ImplementationStatus::Completed;
        state.pr_state = Some(PullRequestState::Merged);
        state.merged_commit = state.verified_head.clone();
        save(&state_dir(&s.repo, &s.ticket).unwrap(), &state).unwrap();
        state
    }

    #[test]
    fn cleanup_reclaims_ignored_builds_keeps_evidence_and_is_idempotent() {
        let s = Sandbox::new();
        let state = completed_cleanup_fixture(&s);
        fs::write(common(&s.repo).unwrap().join("info/exclude"), "target/\n").unwrap();
        fs::create_dir_all(state.worktree.join("target/debug")).unwrap();
        fs::write(
            state.worktree.join("target/debug/build-cache"),
            vec![0u8; 1024 * 1024],
        )
        .unwrap();
        let dir = state_dir(&s.repo, &s.ticket).unwrap();
        let evidence = fs::read(dir.join("verified-report.json")).unwrap();
        refresh_pr(&s.repo, &s.ticket, &cleanup_runner()).unwrap();
        let done = load(&s.repo, &s.ticket).unwrap();
        assert!(done.cleanup.completed_at.is_some(), "{:?}", done.cleanup);
        assert!(!state.worktree.exists());
        assert_eq!(
            fs::read(dir.join("verified-report.json")).unwrap(),
            evidence
        );
        assert_eq!(done.status, ImplementationStatus::Completed);
        assert_eq!(board_column(Some(&done), false), 4);
        refresh_pr(&s.repo, &s.ticket, &cleanup_runner()).unwrap();
        assert_eq!(load(&s.repo, &s.ticket).unwrap().cleanup, done.cleanup);
    }

    #[test]
    fn cleanup_never_reclaims_an_unpublished_completion_commit() {
        let s = Sandbox::new();
        let mut state = s.run("complete", Arc::new(AtomicUsize::new(0))).unwrap();
        state.status = ImplementationStatus::Completed;
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
        assert!(
            failed
                .cleanup
                .error
                .as_deref()
                .unwrap()
                .contains("local changes")
        );
        assert_eq!(fs::read_to_string(&draft).unwrap(), "keep this");
        assert_eq!(failed.status, ImplementationStatus::Completed);
        fs::rename(&draft, s.root.join("saved-draft.txt")).unwrap();
        refresh_pr(&s.repo, &s.ticket, &cleanup_runner()).unwrap();
        assert!(
            load(&s.repo, &s.ticket)
                .unwrap()
                .cleanup
                .completed_at
                .is_some()
        );
        assert!(!state.worktree.exists());
    }

    #[test]
    fn cleanup_preserves_changed_head_and_locked_worktree() {
        let s = Sandbox::new();
        let state = completed_cleanup_fixture(&s);
        s.git(
            &s.repo,
            &["worktree", "lock", state.worktree.to_str().unwrap()],
        );
        refresh_pr(&s.repo, &s.ticket, &cleanup_runner()).unwrap();
        assert!(load(&s.repo, &s.ticket).unwrap().cleanup.error.is_some());
        assert!(state.worktree.exists());
        s.git(
            &s.repo,
            &["worktree", "unlock", state.worktree.to_str().unwrap()],
        );
        s.git(
            &state.worktree,
            &["commit", "--allow-empty", "-qm", "new local work"],
        );
        refresh_pr(&s.repo, &s.ticket, &cleanup_runner()).unwrap();
        assert!(
            load(&s.repo, &s.ticket)
                .unwrap()
                .cleanup
                .error
                .unwrap()
                .contains("changed HEAD")
        );
        assert!(state.worktree.exists());
    }

    #[test]
    fn cleanup_preserves_wrong_identity_missing_publication_and_active_work() {
        let s = Sandbox::new();
        let mut state = completed_cleanup_fixture(&s);
        let dir = state_dir(&s.repo, &s.ticket).unwrap();
        let lock = fs::OpenOptions::new()
            .read(true)
            .write(true)
            .open(dir.join("run.lock"))
            .unwrap();
        lock.lock().unwrap();
        refresh_pr(&s.repo, &s.ticket, &cleanup_runner()).unwrap();
        assert!(state.worktree.exists());
        assert!(
            load(&s.repo, &s.ticket)
                .unwrap()
                .cleanup
                .attempted_at
                .is_none()
        );
        drop(lock);
        state.worktree = s.repo.clone();
        save(&dir, &state).unwrap();
        refresh_pr(&s.repo, &s.ticket, &cleanup_runner()).unwrap();
        assert!(
            load(&s.repo, &s.ticket)
                .unwrap()
                .cleanup
                .error
                .unwrap()
                .contains("allocation")
        );
        assert!(s.repo.join(&s.ticket).exists());
        state.merged_commit = None;
        save(&dir, &state).unwrap();
        refresh_pr(&s.repo, &s.ticket, &cleanup_runner()).unwrap();
        assert!(
            load(&s.repo, &s.ticket)
                .unwrap()
                .cleanup
                .error
                .unwrap()
                .contains("No confirmed")
        );
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
        assert_eq!(state.status, ImplementationStatus::AwaitingReview);
        assert!(state.worktree.join("missing-file").exists());
    }

    #[test]
    fn verification_worktree_path_survives_cd_and_spaces() {
        let s = Sandbox::new();
        let cwd = s
            .root
            .join(".packet-worktrees")
            .join(crate::persistence::project_slug(&s.repo))
            .join("worktree with spaces");
        fs::create_dir_all(cwd.parent().unwrap()).unwrap();
        s.git(
            &s.repo,
            &[
                "worktree",
                "add",
                "--quiet",
                "-b",
                "packet/verify-path-test",
                cwd.to_str().unwrap(),
            ],
        );
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
            assert_eq!(result.status, ImplementationStatus::AwaitingReview);
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
        assert_eq!(result.status, ImplementationStatus::Completed);
        assert!(result.pr_url.is_none());
        assert!(
            result.cleanup.completed_at.is_some(),
            "{:?}",
            result.cleanup
        );
        assert!(!result.worktree.exists());
        let worktrees = s.git(&s.repo, &["worktree", "list", "--porcelain"]);
        assert_eq!(
            worktrees
                .lines()
                .filter(|line| line.starts_with("worktree "))
                .count(),
            1
        );
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
        result.status = ImplementationStatus::Publishing;
        save(&state_dir(&s.repo, &s.ticket).unwrap(), &result).unwrap();
        assert_eq!(run().unwrap().status, ImplementationStatus::Completed);
        assert_eq!(calls.load(Ordering::SeqCst), 1);
        assert!(!s.root.join("pr-created").exists());
    }

    #[test]
    fn resumed_worker_receives_the_submitted_task_decision() {
        struct Capture {
            prompt: Arc<std::sync::Mutex<String>>,
        }
        impl AiHarness for Capture {
            fn label(&self) -> String {
                "capture".into()
            }
            fn check_available(&self) -> Result<String, AppError> {
                Ok("capture".into())
            }
            fn execute(
                &self,
                req: &PlanningRequest,
            ) -> Result<crate::harness::HarnessOutcome, AppError> {
                *self.prompt.lock().unwrap() = req.prompt_body.clone();
                Fixture {
                    mode: "complete",
                    calls: Arc::new(AtomicUsize::new(0)),
                }
                .execute(req)
            }
        }
        let s = Sandbox::new();
        let prompt = Arc::new(std::sync::Mutex::new(String::new()));
        let (tx, _rx) = mpsc::channel();
        run_with_project_options(
            &s.repo,
            &s.repo,
            &s.ticket,
            RunOptions {
                harness: &Capture {
                    prompt: prompt.clone(),
                },
                cancel: Arc::new(AtomicBool::new(false)),
                progress: tx,
                gh: "must-not-run-gh",
                publication_mode: PublicationMode::AutoPublish,
                require_independent_checks: false,
                user_context: Some(
                    "I choose option (b): reissue the corrected footprint predicate.",
                ),
                auto_publish_gate: None,
            },
        )
        .unwrap();
        let recorded = prompt.lock().unwrap();
        assert!(recorded.contains("LATEST SUBMITTED USER RESPONSE FOR THIS TASK"));
        assert!(
            recorded.contains("I choose option (b): reissue the corrected footprint predicate.")
        );
        assert!(recorded.contains("not proof that the ledger was changed"));
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
        assert_eq!(result.status, ImplementationStatus::Completed);
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
        assert_eq!(result.status, ImplementationStatus::Completed);
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
        assert_eq!(result.status, ImplementationStatus::Completed);
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
            status: ReportStatus::Complete,
            blocker_disposition: BlockerDisposition::None,
            summary: "Done".into(),
            acceptance_criteria: vec![report::Criterion {
                criterion: "One".into(),
                evidence: "Proof".into(),
            }],
            verification: vec!["cargo test".into()],
            remaining: vec![],
            human_choices: vec![],
        };
        assert!(validate_report(&report, "## Acceptance criteria\n- One\n- Two\n").is_err());
    }
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
        assert_eq!(retain_suffix("0123456789", 4), "\u{2026}6789".to_string());
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
