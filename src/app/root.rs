//! `PacketApp`: eframe root. Owns the connect screen and the connected
//! screen; drains the turn bus; delegates all pixels to `crate::ui`.

use std::time::{Duration, Instant};

use eframe::{App, Frame};

#[cfg(test)]
use crate::app::dialogs::DlgBrowse;
use crate::app::dialogs::{self, DlgImport, DlgMcp, DlgSettings};
use crate::app::session::{self, Project};
use crate::app::welcome;
use crate::core::implementation::{ImplementationStatus, PullRequestState};
use crate::core::turn::{TurnController, TurnEvt, TurnOutcome};
use crate::domain::chatlog::{ChatMessage, ChatRole};
use crate::domain::item::OpenItem;
use crate::domain::user::CurrentUser;
use crate::harness::PiHarness;
use crate::ui::{Surface, ToastQueue};

#[cfg(test)]
#[path = "conversation_tests.rs"]
mod conversation_tests;

#[path = "feature_approval.rs"]
mod feature_approval;

mod attention;
mod implementation_controller;
mod implementation_decision;
mod repository_switcher;
mod requested_action;
#[cfg(test)]
#[path = "root/task_detail_tests.rs"]
mod task_detail_tests;
mod ui_actions;

/// Root of the packet app.
pub struct PacketApp {
    task_harness: Option<Box<dyn crate::harness::AiHarness>>,
    pending_feature_generation: Option<(std::path::PathBuf, String, String)>,
    screen: Screen,
    /// Test-only spawn seam: when `Some`, [`Self::open_workspace`] launches
    /// this binary instead of resolving the running executable. Shipping
    /// code never writes it (`Default` installs `None`); only `#[cfg(test)]`
    /// fixtures aim it at a benign helper.
    spawn_target_override: Option<std::path::PathBuf>,
    dialog: Option<Dialog>,
    toasts: ToastQueue,
    conn_path: String,
    /// Pasted GitHub URL on the connect card. Per-launch state only (no
    /// persistence); preserved across a FAILED connect so the operator can
    /// fix a typo, cleared only after a successful connect.
    conn_github: String,
    conn_error: Option<String>,
    last_git_refresh: Instant,
    display_refresh: Option<std::thread::JoinHandle<DisplayRefresh>>,
    /// In-flight GitHub clone worker (spawned by [`Self::begin_clone`],
    /// drained at the top of [`Self::tick`]). `None` when idle.
    clone_job: Option<CloneJob>,
    /// Test-only seam: when `Some`, [`Self::begin_clone`] hands the worker
    /// THIS computation instead of the real `welcome::perform_clone`, so
    /// dispatch can be proven hermetically (no network, no real git
    /// process). Shipping code never writes it (`Default` installs `None`).
    clone_computation_override: Option<CloneWorkerCalc>,
    /// Cached routing identity (rebuilt after connect/adoption/settings).
    cached_user: CurrentUser,
    attention: std::collections::BTreeMap<std::path::PathBuf, attention::Status>,
    #[cfg(test)]
    attention_fixture: std::collections::BTreeMap<String, crate::core::attention::Brief>,
    /// Synthesized ownership-gap items for the side pane.
    synth: Vec<OpenItem>,
}

/// Application composition boundary for AI work. Pi is the configured MVP
/// backend; tests and future settings may supply a different implementation.
pub(super) fn configured_harness(
    override_harness: &mut Option<Box<dyn crate::harness::AiHarness>>,
) -> Box<dyn crate::harness::AiHarness> {
    override_harness
        .take()
        .unwrap_or_else(|| Box::new(PiHarness))
}

enum Screen {
    Welcome,
    Connected(Box<Project>),
}

fn persist_automation_settings(project: &mut Project) -> Result<(), String> {
    let temporary_lock = if project.queue_lock.is_none() {
        Some(
            crate::core::implementation_queue::Queue::acquire(&project.state.repo_root)
                .map_err(|error| error.to_string())?,
        )
    } else {
        None
    };
    project.queue.last_error.clear();
    let result = project
        .queue
        .save(&project.state.repo_root)
        .map_err(|error| error.to_string());
    drop(temporary_lock);
    if !project.queue.running && project.active_implementations.is_empty() {
        project.queue_lock = None;
    }
    result
}

struct DisplayRefresh {
    repo: std::path::PathBuf,
    workflow: crate::core::workflow::Workflow,
    git: crate::core::gitops::GitSnapshot,
    documents: Vec<crate::artifacts::task_docs::TaskDocument>,
    implementations:
        std::collections::BTreeMap<String, crate::core::implementation::Implementation>,
    previous_implementations:
        std::collections::BTreeMap<String, crate::core::implementation::Implementation>,
    activity: Vec<(String, crate::harness::LiveProgress)>,
}

/// One in-flight GitHub clone launched from the connect card. The worker
/// owns the FETCH only; validation, bootstrap and hydration still belong
/// to the single connect authority ([`PacketApp::submit_connect`]).
struct CloneJob {
    /// Badge text for the card's status line: "github.com/{owner}/{repo}".
    url_display: String,
    /// Repository segment as pasted (drives the "Cloning {repo} …" line).
    repo: String,
    join: std::thread::JoinHandle<Result<std::path::PathBuf, crate::error::AppError>>,
}

/// Signature of the stand-in computation for the clone worker (see
/// [`PacketApp::clone_computation_override`]): canonical url + repo
/// segment -> the destination placed on success. Behind an Arc so a test
/// fixture can be shared with the spawned thread.
type CloneWorkerCalc = std::sync::Arc<
    dyn Fn(String, String) -> Result<std::path::PathBuf, crate::error::AppError> + Send + Sync,
>;

fn has_current_task_batch(project: &Project) -> bool {
    if let Some((id, _)) = &project.state.active_feature {
        let tagged = project
            .task_documents
            .iter()
            .filter(|doc| !doc.path.ends_with("/README.md"))
            .filter_map(|doc| {
                doc.text
                    .lines()
                    .find_map(|line| line.strip_prefix("Feature ID: "))
                    .map(|feature| (feature, doc))
            });
        let docs = tagged.collect::<Vec<_>>();
        if !docs.is_empty() {
            return docs.iter().any(|(feature, doc)| {
                feature == id
                    && project
                        .state
                        .workflow
                        .task_batches
                        .iter()
                        .any(|batch| doc.path.starts_with(&format!("{}/", batch.directory)))
            });
        }
    }
    project
        .task_documents
        .iter()
        .any(|doc| !doc.path.ends_with("/README.md"))
        && project.state.workflow.brief.as_ref().is_none_or(|brief| {
            project
                .state
                .workflow
                .task_batches
                .last()
                .is_some_and(|batch| batch.feature == brief.feature_name)
        })
}

enum Dialog {
    Import(DlgImport),
    Settings(DlgSettings),
    Mcp(DlgMcp),
    #[cfg(test)]
    Browse(DlgBrowse),
}

/// Native window options for [`eframe::run_native`]. The minimum stays below
/// the compact-layout breakpoint so the stacked workspace is reachable on
/// smaller displays.
pub fn options() -> eframe::NativeOptions {
    let mut vp = egui::ViewportBuilder::default();
    vp = vp
        .with_inner_size([1480.0, 900.0])
        .with_min_inner_size([360.0, 480.0]);
    eframe::NativeOptions {
        viewport: vp,
        ..Default::default()
    }
}

impl Default for PacketApp {
    fn default() -> Self {
        let toasts = ToastQueue::default();

        Self {
            task_harness: None,
            pending_feature_generation: None,
            screen: Screen::Welcome,
            spawn_target_override: None,
            dialog: None,
            toasts,
            conn_path: std::env::current_dir()
                .map(|p| p.to_string_lossy().into_owned())
                .unwrap_or_default(),
            conn_github: String::new(),
            conn_error: None,
            last_git_refresh: Instant::now(),
            display_refresh: None,
            clone_job: None,
            clone_computation_override: None,
            cached_user: CurrentUser::new("", Vec::new()),
            attention: Default::default(),
            #[cfg(test)]
            attention_fixture: Default::default(),
            synth: Vec::new(),
        }
    }
}

// ------------------------------------------------------------------------ tick
impl PacketApp {
    fn tick(&mut self, _dt: f32, ctx: &egui::Context) {
        self.poll_attention(ctx);
        // Clone job: drain BEFORE Phase 1's screen borrow so a finished
        // worker can refill `conn_path` and drive submit_connect — the
        // single connect authority. Unfinished workers ride back until
        // they settle (poll rhythm: take → is_finished → put back).
        if let Some(job) = self.clone_job.take() {
            if job.join.is_finished() {
                match job.join.join() {
                    Err(_) => {
                        self.conn_error =
                            Some("The clone worker stopped unexpectedly. Try again.".into());
                    }
                    Ok(Err(e)) => {
                        // Failed clone: `conn_github` is PRESERVED so the
                        // operator can correct and retry; no connect runs.
                        self.conn_error = Some(Self::conn_banner(&e));
                    }
                    Ok(Ok(dest)) => {
                        // Defensive: no navigation exists out of Welcome
                        // while a job runs, so a non-Welcome screen can
                        // only mean something odd — discard silently.
                        if matches!(self.screen, Screen::Welcome) {
                            self.conn_error = None;
                            self.conn_path = dest.to_string_lossy().into_owned();
                            self.submit_connect();
                        }
                    }
                }
            } else {
                self.clone_job = Some(job);
            }
        }
        // Phase 1: drain pending turn events (borrows `self.screen` only).
        let mut outcome: Option<TurnOutcome> = None;
        let mut task_outcomes = Vec::new();
        if let Screen::Connected(project) = &mut self.screen {
            project.bind_task_conversation_identities();
            project.task_chats.ensure_loaded(&project.chat_slug);
            project
                .activity
                .pending
                .extend(project.task_chats.take_updates());
            project.activity.ensure_overall();
            for (key, ctrl) in &project.task_turns {
                for _ in 0..64 {
                    match ctrl.poll(Duration::ZERO) {
                        Some(TurnEvt::Progress(progress)) => {
                            project
                                .task_live
                                .entry(key.clone())
                                .or_default()
                                .update(progress);
                            project
                                .activity
                                .conversations
                                .entry(key.clone())
                                .or_default()
                                .update(Default::default());
                        }
                        Some(TurnEvt::Done(outcome)) => {
                            task_outcomes.push((key.clone(), *outcome));
                            break;
                        }
                        None => break,
                    }
                }
            }
            if let Some(ctrl) = &project.active_turn {
                for _ in 0..64 {
                    let Some(evt) = ctrl.poll(Duration::ZERO) else {
                        break;
                    };
                    match evt {
                        TurnEvt::Progress(progress) => {
                            project
                                .activity
                                .overall
                                .as_mut()
                                .unwrap()
                                .update(Default::default());
                            let key = project
                                .task_chats
                                .active
                                .clone()
                                .unwrap_or_else(|| "__main".into());
                            project
                                .activity
                                .conversations
                                .entry(key)
                                .or_default()
                                .update(Default::default());
                            project.live_progress.update(progress);
                        }
                        TurnEvt::Done(o) => {
                            outcome = Some(*o);
                            break;
                        }
                    }
                }
            }
        }
        if let Screen::Connected(project) = &mut self.screen {
            let mut finished = Vec::new();
            for (ticket, ctrl) in &project.active_implementations {
                for _ in 0..64 {
                    match ctrl.poll() {
                        Some(crate::core::implementation::Event::Progress(p)) => {
                            project
                                .activity
                                .overall
                                .as_mut()
                                .unwrap()
                                .update(Default::default());
                            {
                                project
                                    .activity
                                    .tasks
                                    .entry(ticket.clone())
                                    .or_default()
                                    .update(p);
                            }
                            project.activity.mark_ticket_dirty(ticket);
                        }
                        Some(crate::core::implementation::Event::Done(result)) => {
                            finished.push((ticket.clone(), *result));
                            break;
                        }
                        None => break,
                    }
                }
            }
            for (ticket, result) in finished {
                project.active_implementations.remove(&ticket);
                project.queue.in_flight.remove(&ticket);
                if project.queue.current_ticket.as_ref() == Some(&ticket) {
                    project.queue.current_ticket = None;
                }
                {
                    if let Some(progress) = project.activity.tasks.get_mut(&ticket) {
                        progress.telemetry.finished_ms =
                            Some(chrono::Utc::now().timestamp_millis());
                        progress.activity = Some(match &result {
                            Ok(record) => record.status.label().to_owned(),
                            Err(_) => "Needs attention".into(),
                        });
                    }
                    project.save_task_activity(&ticket);
                    project.activity.dirty_tickets.remove(&ticket);
                }
                project.refresh_implementations();
                project.last_pr_refresh = None;
                let cleanup_note = result
                    .as_ref()
                    .ok()
                    .and_then(|record| record.cleanup.error.clone());
                let mut text = match result {
                    Ok(record) => {
                        project
                            .implementation_states
                            .insert(ticket.clone(), record.clone());
                        if !record.auto_merge || record.status != ImplementationStatus::Completed {
                            project.queue.running = false;
                        }
                        if crate::core::implementation::permits_evidence_only_completion(
                            &record.ticket_text,
                        ) {
                            format!(
                                "Evidence-only task verified against {} at {}. {}",
                                record.base,
                                record.merged_commit.unwrap_or_default(),
                                if project.queue.running {
                                    "Continuing the Auto queue."
                                } else {
                                    "Queue paused."
                                }
                            )
                        } else if record.auto_merge {
                            format!(
                                "Task merged into {} at {}. {}",
                                record.base,
                                record.merged_commit.unwrap_or_default(),
                                if project.queue.running {
                                    "Continuing the Auto queue."
                                } else {
                                    "Queue paused."
                                }
                            )
                        } else if record.status == ImplementationStatus::ReadyToPublish {
                            "Implementation verified and saved locally. Auto Publish is off, so nothing was shared. Choose Share verified work for review when you are ready.".into()
                        } else {
                            format!(
                                "Implementation verified. Pull request: {}",
                                record.pr_url.unwrap_or_default()
                            )
                        }
                    }
                    Err(error) => {
                        project.queue.blocked.insert(ticket.clone(), error.clone());
                        project.queue.last_error = error.message.clone();
                        if project
                            .queue
                            .recoverable_tickets(&project.task_documents)
                            .contains(&ticket)
                        {
                            "Recoverable orchestration failure; automatically resuming preserved task work.".into()
                        } else {
                            format!(
                                "The task needs attention after automatic recovery. Its work is preserved. Failure: {}",
                                error.message
                            )
                        }
                    }
                };
                if let Some(error) = cleanup_note {
                    text.push_str(&format!("\nTask completed, but worktree cleanup needs attention: {error}. Cleanup will retry automatically."));
                }
                if project.queue_lock.is_some()
                    && let Err(error) = project.queue.save(&project.state.repo_root)
                {
                    project.queue.running = false;
                    project.queue.last_error.push_str(&format!("\nCannot save queue: {error}. Check disk space and permissions; this failure may not survive a restart."));
                }
                if !project.queue.running && project.active_implementations.is_empty() {
                    project.queue_lock = None;
                }
                project.activity.pending.push(text.clone());
                project.remember_chat(vec![ChatMessage::new(ChatRole::System, text, None)]);
                project.refresh_git();
            }
        }
        if let Screen::Connected(project) = &mut self.screen {
            if let Some(errors) = project
                .pr_refresh
                .as_ref()
                .and_then(|refresh| refresh.poll())
            {
                project.pr_refresh = None;
                project.refresh_implementations();
                if errors.is_empty()
                    && project
                        .queue
                        .last_error
                        .starts_with("Task maintenance failed for ")
                {
                    project.queue.last_error.clear();
                }
                for (ticket, error) in errors {
                    if let Some(state) = project.implementation_states.get_mut(&ticket) {
                        if state.status == ImplementationStatus::Completed {
                            state.cleanup.error = Some(error.clone());
                        } else {
                            state.pr_check_error = Some(error.clone());
                        }
                    }
                    project.queue.last_error =
                        format!("Task maintenance failed for {ticket}: {error}");
                }
            }
            if project.pr_refresh.is_none()
                && project
                    .last_pr_refresh
                    .is_none_or(|last| last.elapsed() >= Duration::from_secs(60))
            {
                let mut states = project
                    .implementation_states
                    .iter()
                    .filter(|(_, state)| {
                        (state.status != ImplementationStatus::Completed
                            && state.pr_url.is_some()
                            && state.pr_state != Some(PullRequestState::Merged))
                            || (state.status == ImplementationStatus::Completed
                                && state.cleanup.completed_at.is_none())
                    })
                    .collect::<Vec<_>>();
                states.sort_by_key(|(_, state)| {
                    if state.status == ImplementationStatus::Completed {
                        &state.cleanup.attempted_at
                    } else {
                        &state.pr_check_attempted_at
                    }
                });
                let tickets = states
                    .into_iter()
                    .map(|(ticket, _)| ticket.clone())
                    .collect::<Vec<_>>();
                if !tickets.is_empty() {
                    project.pr_refresh = Some(crate::core::implementation::PrRefresh::start(
                        project.state.repo_root.clone(),
                        tickets,
                    ));
                }
                project.last_pr_refresh = Some(Instant::now());
            }
        }
        // Phase 2: apply a completed turn. The project is detached first so
        // the `&mut self` work (caches, toasts) cannot alias `self.screen`.
        if let Some(o) = outcome {
            let applied = matches!(&o, TurnOutcome::Applied { .. });
            let slot = std::mem::replace(&mut self.screen, Screen::Welcome);
            if let Screen::Connected(mut project) = slot {
                let requested_action = self.adopt_turn(&mut project, o);
                self.screen = Screen::Connected(project);
                self.continue_feature_generation(applied);
                if let Some(action) = requested_action {
                    requested_action::dispatch(self, action);
                }
            }
        }
        for (key, outcome) in task_outcomes {
            let slot = std::mem::replace(&mut self.screen, Screen::Welcome);
            if let Screen::Connected(mut project) = slot {
                project.task_turns.remove(&key);
                project.task_live.remove(&key);
                // Adoption routes messages to this conversation; preserve the
                // independent Main Chat worker and its live output.
                let main = project.active_turn.take();
                let live = std::mem::take(&mut project.live_progress);
                project.task_chats.active = Some(key);
                let _ = self.adopt_turn(&mut project, outcome);
                project.active_turn = main;
                project.live_progress = live;
                self.screen = Screen::Connected(project);
            }
        }
        // Poll only: repository subprocesses and reads must not stall input frames.
        if self
            .display_refresh
            .as_ref()
            .is_some_and(|job| job.is_finished())
        {
            if let Ok(result) = self.display_refresh.take().unwrap().join()
                && let Screen::Connected(p) = &mut self.screen
                && p.state.repo_root == result.repo
                && p.state.workflow == result.workflow
                && p.implementation_states == result.previous_implementations
            {
                p.git = result.git;
                p.task_documents = result.documents;
                p.adopt_implementations(result.implementations);
                for (ticket, activity) in result.activity {
                    p.activity.tasks.entry(ticket).or_insert(activity);
                }
                (self.cached_user, self.synth) = Self::derive_caches(p);
            }
            self.last_git_refresh = Instant::now();
        }
        if self.display_refresh.is_none()
            && self.last_git_refresh.elapsed() > Duration::from_secs(3)
            && let Screen::Connected(p) = &self.screen
        {
            let repo = p.state.repo_root.clone();
            let workflow = p.state.workflow.clone();
            let previous_implementations = p.implementation_states.clone();
            let ctx = ctx.clone();
            self.display_refresh = Some(std::thread::spawn(move || {
                let git = crate::core::gitops::snapshot(&repo);
                let documents = crate::artifacts::task_docs::load_board(&repo, &workflow);
                let implementations = crate::core::implementation::load_board_states(&repo);
                let activity = implementations
                    .keys()
                    .filter_map(|ticket| {
                        crate::core::implementation::load_activity(&repo, ticket)
                            .map(|p| (ticket.clone(), p))
                    })
                    .collect();
                ctx.request_repaint();
                DisplayRefresh {
                    repo,
                    workflow,
                    git,
                    documents,
                    implementations,
                    previous_implementations,
                    activity,
                }
            }));
        }
        if let Screen::Connected(project) = &mut self.screen {
            // Persist only tickets whose activity actually moved since the
            // last flush, on a 2 s cadence (was: every active ticket, every
            // 2 s, whether changed or not).
            if !project.activity.dirty_tickets.is_empty()
                && project
                    .activity
                    .last_save
                    .is_none_or(|last| last.elapsed() >= Duration::from_secs(2))
            {
                for ticket in project.activity.take_dirty_tickets() {
                    project.save_task_activity(&ticket);
                }
                project.activity.last_save = Some(Instant::now());
            }
            let mut result = None;
            if let Some(manager) = &project.activity.manager {
                for _ in 0..64 {
                    let Some(progress) = manager.progress() else {
                        break;
                    };
                    project.live_progress.update(progress);
                    project
                        .activity
                        .overall
                        .as_mut()
                        .unwrap()
                        .update(Default::default());
                    project
                        .activity
                        .conversations
                        .entry("__main".into())
                        .or_default()
                        .update(Default::default());
                }
                result = manager.result();
            }
            if let Some(result) = result {
                project.activity.manager = None;
                project.live_progress = Default::default();
                match result {
                    Ok(text) => project.remember_chat(vec![ChatMessage::new(ChatRole::Agent, text, None)]),
                    Err(_) => project.remember_chat(vec![ChatMessage::new(ChatRole::System, "Project-manager update unavailable after retry; task work continues. You can still send a message.", None)]),
                }
            }
            // Stalled-worker patrol: surfaced and acted upon at most once per
            // cooldown period, so a queue stuck on hung workers cannot drive
            // an endless stream of background manager LLM turns.
            if super::manager::patrol_note_due(
                project.active_implementations.len(),
                project.activity.pending.len(),
                project.activity.last_update,
                project.activity.last_patrol_note,
                Instant::now(),
            ) {
                project.activity.pending.push("The worker is still running. No completion is confirmed; review the current task states and help the user with the next eligible planning decision without inventing progress.".into());
                project.activity.last_patrol_note = Some(Instant::now());
            }
            if super::manager::patrol_manager_due(
                project.queue.auto_plan,
                project.active_turn.is_some(),
                project.activity.manager.is_some(),
                project.activity.pending.len(),
                project.activity.last_update,
                Instant::now(),
            ) {
                let events = std::mem::take(&mut project.activity.pending);
                let harness = configured_harness(&mut self.task_harness);
                project.activity.manager =
                    Some(super::manager::Manager::start(project, &events, harness));
                project.activity.last_update = Some(Instant::now());
                project.live_progress = crate::harness::LiveProgress {
                    activity: Some("Reviewing project progress…".into()),
                    ..Default::default()
                };
            }
        }
        self.advance_auto_publish();
        self.advance_auto_queue();
        self.advance_reconciliation();
        self.advance_investigation();
        let period = match &self.screen {
            Screen::Connected(p)
                if (p.active_turn.is_some()
                    || !p.task_turns.is_empty()
                    || !p.active_implementations.is_empty()
                    || p.reconciliation.is_running()
                    || p.investigation.is_some()
                    || p.activity.manager.is_some()) =>
            {
                Duration::from_millis(120)
            }
            _ => Duration::from_millis(800),
        };
        ctx.request_repaint_after(period);
    }

    fn advance_reconciliation(&mut self) {
        let Screen::Connected(project) = &mut self.screen else {
            return;
        };
        let event =
            project
                .reconciliation
                .advance(&project.state, &mut self.task_harness, Instant::now());
        let mut toast = None;
        match event {
            Some(crate::app::reconciliation_lifecycle::Event::Started { feature_id }) => {
                project.activity.pending.push(format!(
                    "All tasks for {feature_id} have merged; checking actual implementation against the approved feature."
                ));
            }
            Some(crate::app::reconciliation_lifecycle::Event::Completed {
                feature_id,
                state,
                message,
            }) => {
                project.state = state;
                project.task_documents = crate::artifacts::task_docs::load_board(
                    &project.state.repo_root,
                    &project.state.workflow,
                );
                project.refresh_git();
                project
                    .activity
                    .pending
                    .push(format!("Reconciled {feature_id}: {message}"));
                toast = Some((true, format!("Reconciled {feature_id}")));
            }
            Some(crate::app::reconciliation_lifecycle::Event::Deferred {
                feature_id,
                error,
                state,
            }) => {
                if let Some(state) = state {
                    project.state = state;
                }
                project.activity.pending.push(format!(
                    "Reconciliation of {feature_id} deferred - the project is still moving; Packet will check again shortly. ({error})"
                ));
            }
            Some(crate::app::reconciliation_lifecycle::Event::Failed {
                feature_id,
                error,
                state,
            }) => {
                if let Some(state) = state {
                    project.state = state;
                }
                project.activity.pending.push(format!(
                    "Reconciliation of {feature_id} needs attention: {error}"
                ));
                toast = Some((false, format!("Reconciliation needs attention: {error}")));
            }
            Some(crate::app::reconciliation_lifecycle::Event::ProbeFailed {
                feature_id,
                error,
            }) => {
                project.activity.pending.push(format!(
                    "Reconciliation of {feature_id} needs attention: {error}"
                ));
            }
            None => {}
        }
        if let Some((success, message)) = toast {
            if success {
                self.toasts.success(message);
            } else {
                self.toasts.warning(message);
            }
        }
    }

    fn advance_investigation(&mut self) {
        let Screen::Connected(project) = &mut self.screen else {
            return;
        };
        let mut cancelled = false;
        if let Some(controller) = &project.investigation {
            let item_id = controller.item_id.clone();
            let mut finished = None;
            for _ in 0..64 {
                match controller.poll() {
                    Some(crate::core::investigation::Event::Progress(update)) => {
                        project
                            .activity
                            .overall
                            .as_mut()
                            .unwrap()
                            .update(Default::default());
                        project
                            .activity
                            .tasks
                            .entry(item_id.clone())
                            .or_default()
                            .update(update);
                        project.activity.mark_ticket_dirty(&item_id);
                    }
                    Some(crate::core::investigation::Event::Done(result)) => {
                        cancelled = controller.cancellation_requested();
                        finished = Some(*result);
                        break;
                    }
                    None => break,
                }
            }
            if let Some(result) = finished {
                project.investigation = None;
                if let Some(progress) = project.activity.tasks.get_mut(&item_id) {
                    progress.telemetry.finished_ms = Some(chrono::Utc::now().timestamp_millis());
                }
                project.save_task_activity(&item_id);
                project.activity.dirty_tickets.remove(&item_id);
                if cancelled {
                    if let Ok(current) =
                        crate::core::state::PlannerState::load(&project.state.repo_root)
                    {
                        project.state = current;
                    }
                    if let Some(progress) = project.activity.tasks.get_mut(&item_id) {
                        progress.activity = Some("Paused because Auto Plan is off".into());
                    }
                    project.investigation_cooldown_until = None;
                    project.save_task_activity(&item_id);
                } else {
                    match result {
                        Ok((state, message)) => {
                            project.state = state;
                            project.investigation_cooldown_until = None;
                            project
                                .activity
                                .pending
                                .push(format!("Agent item {item_id}: {message}"));
                        }
                        Err(error) => {
                            if let Ok(current) =
                                crate::core::state::PlannerState::load(&project.state.repo_root)
                            {
                                project.state = current;
                            }
                            let error = error.to_string();
                            if error.starts_with(crate::core::reconciliation::DEFER_PREFIX) {
                                // Benign contention: keep the item open and let
                                // it retry after a quiet stretch; no alarm.
                                project.investigation_cooldown_until =
                                    Some(Instant::now() + Duration::from_secs(300));
                                project.activity.pending.push(format!(
                                "Investigation of {item_id} deferred - the project is still moving; Packet will try again shortly."
                            ));
                            } else {
                                project.investigation_attempted.insert(item_id.clone());
                                project
                                    .activity
                                    .tasks
                                    .entry(item_id.clone())
                                    .or_default()
                                    .activity = Some(format!("Needs attention: {error}"));
                                project
                                    .activity
                                    .pending
                                    .push(format!("Agent item {item_id} needs attention: {error}"));
                            }
                            project.save_task_activity(&item_id);
                        }
                    }
                }
            }
        }
        if project.investigation.is_some()
            || !project.queue.auto_plan
            || project
                .investigation_cooldown_until
                .is_some_and(|until| until > Instant::now())
        {
            return;
        }
        let next = project
            .state
            .items
            .iter()
            .filter(|item| {
                item.authority == crate::domain::Authority::Agent
                    && item.status == crate::domain::ItemStatus::Open
                    && !project.task_turns.contains_key(item.conversation_key())
                    && !project.investigation_attempted.contains(&item.id)
            })
            .min_by_key(|item| (item.priority.rank(), &item.id));
        if let Some(item) = next {
            let item_id = item.id.clone();
            project
                .activity
                .tasks
                .entry(item_id.clone())
                .or_default()
                .telemetry
                .started_ms = Some(chrono::Utc::now().timestamp_millis());
            project
                .activity
                .tasks
                .entry(item_id.clone())
                .or_default()
                .activity = Some("Investigating repository evidence…".into());
            project.activity.mark_ticket_dirty(&item_id);
            let harness = configured_harness(&mut self.task_harness);
            project.investigation = Some(crate::core::investigation::Controller::start(
                project.state.clone(),
                item_id,
                harness,
            ));
        }
    }

    /// Derived caches read by other layers through `Surface`.
    fn refresh_derived(&mut self, project: &Project) {
        (self.cached_user, self.synth) = Self::derive_caches(project);
    }

    /// Pure derivation of the display caches, kept separated from `&mut
    /// self` so `tick` can compute them without double-borrowing.
    ///
    /// The cached routing identity is THE SEATED OPERATOR (FR-13), so the
    /// panel highlight, the chat eligibility display, and the turn pipeline
    /// all judge one and the same identity.
    fn derive_caches(project: &Project) -> (CurrentUser, Vec<OpenItem>) {
        (
            project.state.effective_user(),
            crate::core::ownership::synthesize_for_state(&project.state),
        )
    }

    fn adopt_turn(
        &mut self,
        project: &mut Project,
        outcome: TurnOutcome,
    ) -> Option<crate::harness::RequestedAction> {
        let requested_action = if project.task_chats.active.is_none() {
            match &outcome {
                TurnOutcome::Applied { normalized, .. } => normalized.requested_action.clone(),
                _ => None,
            }
        } else {
            None
        };
        {
            let work_key = project
                .task_chats
                .active
                .clone()
                .or_else(|| project.active_planning_work.take());
            if let Some(key) = work_key {
                if let Some(work) = project.planning_work.iter_mut().find(|w| w.key == key) {
                    match &outcome {
                        TurnOutcome::Applied { normalized, .. } => {
                            work.feature = normalized
                                .document_updates
                                .iter()
                                .find_map(|(id, _)| id.strip_prefix("feature:").map(str::to_owned))
                                .or(work.feature.clone());
                            work.column = if work.feature.is_some() { 1 } else { 4 };
                            work.detail = normalized.assistant_message.clone();
                        }
                        _ => {
                            work.column = 3;
                            work.detail = "Planning needs attention; continue this request in its conversation.".into();
                        }
                    }
                }
                if let Err(error) = crate::core::planning_work::save(
                    &project.state.repo_root,
                    &project.planning_work,
                ) {
                    self.toasts
                        .danger(format!("Cannot save planning board: {error}"));
                }
            }
        }
        project.activity.ensure_overall();
        project
            .activity
            .overall
            .as_mut()
            .unwrap()
            .update(Default::default());
        let activity_key = project
            .task_chats
            .active
            .clone()
            .unwrap_or_else(|| "__main".into());
        project
            .activity
            .conversations
            .entry(activity_key)
            .or_default()
            .update(Default::default());
        project.active_turn = None;
        project.live_progress = crate::harness::LiveProgress::default();
        match outcome {
            TurnOutcome::Applied {
                state,
                receipt,
                normalized,
                commit_result,
                ..
            } => {
                let previous_batches = project.state.workflow.task_batches.len();
                // Another worker may have committed since this outcome was
                // queued. Adopt current disk truth, never an older snapshot.
                project.state =
                    crate::core::state::PlannerState::load(&state.repo_root).unwrap_or(*state);
                project.task_documents = crate::artifacts::task_docs::load_board(
                    &project.state.repo_root,
                    &project.state.workflow,
                );
                if project.task_chats.active.is_none() {
                    project.next_question_id = normalized.next_question_id.clone();
                } else if project
                    .next_question_id
                    .as_ref()
                    .is_some_and(|id| !project.state.items.iter().any(|item| &item.id == id))
                {
                    project.next_question_id = None;
                }
                let mut chat = vec![ChatMessage::new(
                    ChatRole::Agent,
                    normalized.assistant_message,
                    normalized.next_question_id.clone(),
                )];
                if !receipt.synthesized_open_items.is_empty() {
                    chat.push(ChatMessage::new(
                        ChatRole::System,
                        format!(
                            "Raised ownership gap(s): {}",
                            receipt.synthesized_open_items.join(", ")
                        ),
                        None,
                    ));
                }
                chat.extend(
                    normalized
                        .warnings
                        .iter()
                        .map(|w| ChatMessage::new(ChatRole::System, w.clone(), None)),
                );
                if project.state.workflow.task_batches.len() > previous_batches
                    && let Some(batch) = project.state.workflow.task_batches.last()
                {
                    chat.push(ChatMessage::new(ChatRole::System, format!("Created {} detailed task stories in {}. Open the Task stories tab to review them.", batch.count, batch.directory), None));
                }
                project.remember_turn_chat(chat);
                if let Some(key) = project.task_chats.active.clone() {
                    project.remember_turn_chat(vec![ChatMessage::new(
                        ChatRole::System,
                        format!(
                            "Task reply applied to planning artifacts. {}",
                            if project
                                .state
                                .resolved_items
                                .iter()
                                .any(|i| i.conversation_key() == key)
                            {
                                "This question is resolved."
                            } else {
                                "See the current task for any remaining questions."
                            }
                        ),
                        Some(key),
                    )]);
                }
                project.refresh_git();
                self.refresh_derived(project);
                match &commit_result {
                    Ok(sha) => self.toasts.success(format!(
                        "Checkpoint {} · {}",
                        sha.chars().take(7).collect::<String>(),
                        receipt.commit_message
                    )),
                    Err(e) => self
                        .toasts
                        .warning(format!("Applied; git checkpoint failed: {}", e.headline())),
                }
            }
            TurnOutcome::Rejected {
                problems,
                final_text,
                ..
            } => {
                let mut chat = vec![ChatMessage::new(
                    ChatRole::System,
                    format!(
                        "⚠ Turn rejected — nothing was written.\n{}",
                        problems.join("\n")
                    ),
                    None,
                )];
                chat.push(ChatMessage::new(
                    ChatRole::Agent,
                    if final_text.trim().is_empty() {
                        "(no legible reply — try again)".to_string()
                    } else {
                        final_text
                    },
                    None,
                ));
                project.remember_turn_chat(chat);
                self.toasts.danger(format!(
                    "Rejected: {}",
                    problems.first().map(String::as_str).unwrap_or("")
                ));
            }
            TurnOutcome::HarnessFailed { error, .. } => {
                project.remember_turn_chat(vec![ChatMessage::new(
                    ChatRole::System,
                    match &error {
                        crate::error::AppError::InvalidResponse { .. } => {
                            format!("Task generation needs attention: {}", error.detail())
                        }
                        _ => format!("Planning stopped: {}", error.headline()),
                    },
                    None,
                )]);
                self.toasts.danger(error.headline());
            }
        }
        project.task_chats.active = None;
        requested_action
    }

    fn submit_task_reply(&mut self, key: &str) {
        let Screen::Connected(project) = &mut self.screen else {
            return;
        };
        if project.task_turns.contains_key(key) {
            return;
        }
        let text = project
            .task_chats
            .drafts
            .get(key)
            .cloned()
            .unwrap_or_default();
        if text.trim().is_empty() {
            return;
        }
        if let Err(error) = crate::core::task_conversation::prompt(&project.state, key, &text, &[])
        {
            self.toasts.warning(error);
            return;
        }
        project.bind_task_conversation_identities();
        project.task_chats.ensure_loaded(&project.chat_slug);
        let user_message = ChatMessage::new(ChatRole::User, &text, Some(key.into()));
        let sent_id = user_message.id.clone();
        if let Err(error) = project
            .task_chats
            .append(&project.chat_slug, key, vec![user_message])
        {
            self.toasts.warning(error);
            return;
        }
        project.activity.pending.push(format!(
            "User replied in task {key}: {}",
            crate::core::context_build::clip(&text, 1600)
        ));
        // append merges the current on-disk history under a lock, so the prompt
        // also sees replies saved by another window since the last refresh.
        let recent_chat = project
            .task_chats
            .messages
            .get(key)
            .into_iter()
            .flatten()
            .filter(|m| m.id != sent_id)
            .map(|m| (format!("{:?}", m.role), m.text.clone()))
            .collect();
        let inputs = crate::core::turn::TurnInputs {
            state: project.state.clone(),
            user_message: text,
            recent_chat,
            purpose: crate::core::workflow::TurnPurpose::Interview,
            comparison_feature: None,
        };
        project.task_chats.drafts.remove(key);
        let harness = configured_harness(&mut self.task_harness);
        project.task_turns.insert(
            key.into(),
            std::rc::Rc::new(TurnController::start_scoped(
                inputs,
                harness,
                Some(key.into()),
            )),
        );
        project.task_live.insert(key.into(), Default::default());
    }

    // ---------------------------------------------------------------- actions
    /// Start a GitHub clone of `raw` (the connect card's URL field). Runs
    /// synchronously: a blank value is a silent no-op, an unparseable URL
    /// sets the banner and spawns NOTHING (no thread, no subprocess —
    /// quirk-bearing strings never reach git), and a valid target hands the
    /// CANONICAL rebuilt url to a background worker that clones into
    /// `$HOME/{repo}`.
    fn begin_clone(&mut self, raw: &str) {
        let url = raw.trim();
        if url.is_empty() {
            return;
        }
        // Defense in depth: the busy card already steals every signal (the
        // primary guard); refuse here too so a stray duplicate can never
        // leak the in-flight JoinHandle nor double-book the scratch name.
        if self.clone_job.is_some() {
            return;
        }
        match welcome::parse_github_url(url) {
            Err(detail) => {
                self.conn_error = Some(format!("Can't clone that URL\n{detail}"));
            }
            Ok(target) => {
                let url_display = format!("github.com/{}/{}", target.owner, target.repo);
                let canonical = target.url.clone();
                let repo = target.repo.clone();
                let job_repo = repo.clone();
                let join = if let Some(compute) = self.clone_computation_override.clone() {
                    std::thread::spawn(move || compute(canonical, job_repo))
                } else {
                    std::thread::spawn(move || welcome::perform_clone(&canonical, &job_repo))
                };
                self.clone_job = Some(CloneJob {
                    url_display,
                    repo,
                    join,
                });
            }
        }
    }

    /// The card's 'Clone' entry point: feed the FIELD's value (trimmed) to
    /// [`Self::begin_clone`].
    fn begin_clone_from_field(&mut self) {
        let raw = self.conn_github.trim().to_string();
        self.begin_clone(&raw);
    }

    /// Single banner formatter for every connect/clone failure (headline
    /// over detail) so the submit path and the clone-completion path
    /// cannot drift apart.
    fn conn_banner(e: &crate::error::AppError) -> String {
        format!("{}\n{}", e.headline(), e.detail())
    }

    fn submit_connect(&mut self) {
        if self.conn_path.trim().is_empty() {
            return;
        }
        match welcome::attempt_connect(&self.conn_path) {
            Ok(mut project) => {
                self.attention.clear();
                self.refresh_derived(&project);
                project.remember_chat(vec![session::welcome_message(&project.state.title)]);
                self.conn_error = None;
                let title = project.state.title.clone();
                self.screen = Screen::Connected(Box::new(project));
                // Success: forget the pasted URL (per-launch state only).
                // A FAILED connect preserves it for typo correction.
                self.conn_github.clear();
                self.toasts.success(format!("Connected to {title}"));
            }
            Err(e) => {
                self.conn_error = Some(Self::conn_banner(&e));
            }
        }
    }

    fn start_turn(&mut self, text: &str) {
        self.start_turn_with_purpose(text, crate::core::workflow::TurnPurpose::Interview);
    }

    fn start_turn_with_purpose(&mut self, text: &str, purpose: crate::core::workflow::TurnPurpose) {
        self.start_turn_for_feature(text, purpose, None);
    }

    pub(super) fn start_comparison_turn(&mut self, feature_id: &str) {
        let request = format!("Compare plans for feature {feature_id}.");
        self.start_turn_for_feature(
            &request,
            crate::core::workflow::TurnPurpose::ComparePlans,
            Some(feature_id),
        );
    }

    fn start_turn_for_feature(
        &mut self,
        text: &str,
        purpose: crate::core::workflow::TurnPurpose,
        comparison_feature: Option<&str>,
    ) {
        let Screen::Connected(project) = &mut self.screen else {
            return;
        };
        if project.active_turn.is_some()
            || (!project.active_implementations.is_empty()
                && purpose == crate::core::workflow::TurnPurpose::GenerateTasks)
        {
            return;
        }
        project.activity.manager = None;
        project.bind_task_conversation_identities();
        project.task_chats.refresh_now(&project.chat_slug);
        project
            .activity
            .pending
            .extend(project.task_chats.take_updates());
        let task_context = project.task_interaction_context(text);
        let recent = project.recent_chat_tuples(6, 1200);
        let key = format!(
            "planning:{}",
            ChatMessage::new(ChatRole::User, text, None).id
        );
        project
            .planning_work
            .push(crate::core::planning_work::Work {
                key: key.clone(),
                title: format!("Plan {}", crate::core::context_build::clip(text, 100)),
                request: text.into(),
                column: 1,
                feature: None,
                detail: "Planning in progress".into(),
            });
        if let Err(error) =
            crate::core::planning_work::save(&project.state.repo_root, &project.planning_work)
        {
            project.planning_work.pop();
            self.toasts
                .danger(format!("Cannot record planning work: {error}"));
            return;
        }
        project.active_planning_work = Some(key);
        project.remember_chat(vec![ChatMessage::new(ChatRole::User, text, None)]);
        let inputs = crate::core::turn::TurnInputs {
            state: project.state.clone(),
            user_message: format!(
                "{text}\n\n{task_context}\n\n[Application project context: active task worker={:?}; auto queue running={}; task count={}; recent task states={:?}. Continue managing the project and engaging this user while the isolated worker handles implementation. Do not claim to steer or stop a worker through prose; task controls manage that. Planning answers may update the specification normally.]",
                project.active_implementations.keys().collect::<Vec<_>>(),
                project.queue.running,
                project.implementation_states.len(),
                project
                    .implementation_states
                    .iter()
                    .rev()
                    .take(5)
                    .map(|(ticket, state)| (ticket, state.status.label()))
                    .collect::<Vec<_>>()
            ),
            recent_chat: recent,
            purpose,
            comparison_feature: comparison_feature.map(str::to_owned),
        };
        let harness = configured_harness(&mut self.task_harness);
        let ctrl = TurnController::start(inputs, harness);
        project.active_turn = Some(std::rc::Rc::new(ctrl));
        project.live_progress = crate::harness::LiveProgress {
            activity: Some("Starting planner…".into()),
            ..Default::default()
        };
    }

    fn disconnect(&mut self) {
        self.pending_feature_generation = None;
        if let Screen::Connected(p) = &mut self.screen {
            for ctrl in p.active_implementations.values() {
                ctrl.request_cancel();
            }
            if p.active_turn.is_some() {
                if let Some(ctrl) = &p.active_turn {
                    ctrl.request_cancel();
                }
                self.toasts
                    .warning(format!("Turn aborted; disconnected from {}", p.state.title));
            }
        }
        self.dialog = None;
        self.synth.clear();
        self.screen = Screen::Welcome;
    }

    /// Workspace menu 'Open workspace': spawn a DETACHED sibling Packet
    /// process that boots to the initial (Welcome) screen.
    ///
    /// Deliberate contract contrast with [`Self::disconnect`]: this hands
    /// out a second window and touches NONE of this session — no
    /// `request_cancel` on the active turn or implementations, no
    /// `screen`/`dialog`/`synth`/`queue` mutation. Spawn + toast only.
    pub fn open_workspace(&mut self) {
        let target: Result<std::path::PathBuf, String> = self
            .spawn_target_override
            .clone()
            .map(Ok)
            .unwrap_or_else(crate::app::spawn::resolve_self_executable);
        match target {
            Ok(bin) => match crate::app::spawn::spawn_sibling(&bin) {
                Ok(()) => self.toasts.info("Opening a new Packet window"),
                Err(message) => self.toasts.warning(message),
            },
            Err(message) => self.toasts.warning(message),
        }
    }

    fn copy_spec_to_clipboard(&mut self) {
        let Screen::Connected(p) = &self.screen else {
            return;
        };
        let text = p
            .live_progress
            .specification
            .as_deref()
            .or(p.state.spec_text.as_deref())
            .unwrap_or_default()
            .to_owned();
        clipboard_put(&text);
        self.toasts.info(format!(
            "Copied {} characters to clipboard",
            text.chars().count()
        ));
    }
}

fn clipboard_put(text: &str) {
    let bins = ["pbcopy", "wl-copy", "xclip"];
    for bin in bins {
        if let Ok(mut child) = std::process::Command::new(bin)
            .stdin(std::process::Stdio::piped())
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .spawn()
        {
            use std::io::Write;
            if let Some(stdin) = child.stdin.as_mut() {
                let _ = stdin.write_all(text.as_bytes());
            }
            if child.wait().map(|s| s.success()).unwrap_or(false) {
                return;
            }
        }
    }
}

// ------------------------------------------------------------------- Surface
impl PacketApp {
    #[cfg(test)]
    fn implementation_capacity(&self) -> bool {
        matches!(&self.screen, Screen::Connected(project)
            if project.active_turn.is_none()
                && project.active_implementations.len() < project.queue.max_parallel.clamp(1, 8))
    }

    #[cfg(test)]
    fn implement_task(&mut self, ticket: String) {
        self.start_implementation(ticket, true);
    }

    fn cancel_task_for(&mut self, ticket: &str) {
        if let Screen::Connected(project) = &mut self.screen {
            project.queue.running = false;
            project.queue.recovery_paused = true;
            if let Some(controller) = project.active_implementations.get(ticket) {
                controller.request_cancel();
            }
            if project.queue_lock.is_some()
                && let Err(error) = project.queue.save(&project.state.repo_root)
            {
                project.queue.last_error = error.to_string();
            }
        }
    }
}

impl Surface for PacketApp {
    fn session_title(&self) -> &str {
        match &self.screen {
            Screen::Connected(p) => p.state.title.as_str(),
            Screen::Welcome => "",
        }
    }

    fn is_git_repo(&self) -> bool {
        matches!(&self.screen, Screen::Connected(_))
    }

    fn registered_repositories(&self) -> Vec<crate::ui::RepositoryChoice> {
        repository_switcher::choices(&self.screen)
    }

    fn git_branch(&self) -> &str {
        match &self.screen {
            Screen::Connected(p) => p.git.branch.as_str(),
            Screen::Welcome => "",
        }
    }

    fn git_head(&self) -> &str {
        match &self.screen {
            Screen::Connected(p) => p.git.head_short.as_str(),
            Screen::Welcome => "",
        }
    }

    fn git_dirty(&self) -> bool {
        match &self.screen {
            Screen::Connected(p) => p.git.dirty > 0,
            Screen::Welcome => false,
        }
    }

    fn chat_messages(&self) -> &[ChatMessage] {
        match &self.screen {
            Screen::Connected(p) => p.chat.as_slice(),
            Screen::Welcome => &[],
        }
    }

    fn chat_draft(&mut self) -> &mut String {
        match &mut self.screen {
            Screen::Connected(p) => &mut p.draft,
            Screen::Welcome => &mut self.conn_path,
        }
    }

    fn task_messages(&self, key: &str) -> &[ChatMessage] {
        match &self.screen {
            Screen::Connected(p) => p
                .task_chats
                .messages
                .get(key)
                .map(Vec::as_slice)
                .unwrap_or(&[]),
            _ => &[],
        }
    }
    fn task_chat_context(&self, key: &str) -> Option<String> {
        let Screen::Connected(p) = &self.screen else {
            return None;
        };
        let (mut context, _) =
            crate::core::task_conversation::presentation(&p.state, &p.task_documents, key)?;
        if let Some(implementation) = p.implementation_states.get(key) {
            context.push_str(&format!(
                "\n\nImplementation: {}\n{}",
                implementation.status,
                crate::core::context_build::clip(&implementation.detail, 1600)
            ));
        }
        Some(context)
    }
    fn task_draft(&mut self, key: &str) -> Option<&mut String> {
        match &mut self.screen {
            Screen::Connected(p) => Some(p.task_chats.drafts.entry(key.into()).or_default()),
            _ => None,
        }
    }
    fn task_chat_active(&self, key: &str) -> bool {
        matches!(&self.screen, Screen::Connected(p) if p.task_turns.contains_key(key))
    }
    fn task_reply_progress(&self, key: &str) -> Option<&crate::harness::LiveProgress> {
        match &self.screen {
            Screen::Connected(p) => p.task_live.get(key),
            _ => None,
        }
    }
    fn task_chat_error(&self) -> Option<&str> {
        match &self.screen {
            Screen::Connected(p) => p.task_chats.error.as_deref(),
            _ => None,
        }
    }
    fn activity_samples(&self, key: Option<&str>) -> Vec<(i64, u64)> {
        let Screen::Connected(p) = &self.screen else {
            return Vec::new();
        };
        if key.is_none()
            && let Some(overall) = &p.activity.overall
        {
            return overall.telemetry.samples.clone();
        }
        let item_id = key
            .and_then(|key| {
                p.state
                    .items
                    .iter()
                    .chain(&p.state.resolved_items)
                    .find(|item| item.conversation_key() == key)
                    .map(|item| item.id.as_str())
            })
            .or(key);
        let mut buckets = std::collections::BTreeMap::<i64, u64>::new();
        for (id, progress) in &p.activity.tasks {
            if item_id.is_none_or(|key| key == id) {
                for (bucket, count) in &progress.telemetry.samples {
                    *buckets.entry(*bucket).or_default() += count;
                }
            }
        }
        for (id, progress) in &p.activity.conversations {
            if key.is_none_or(|key| key == id) {
                for (bucket, count) in &progress.telemetry.samples {
                    *buckets.entry(*bucket).or_default() += count;
                }
            }
        }
        buckets.into_iter().collect()
    }
    fn activity_active(&self, key: &str) -> bool {
        self.implementation_active(key)
            || self.task_chat_active(key)
            || matches!(&self.screen,
            Screen::Connected(p) if p.investigation.as_ref().is_some_and(|run| run.item_id == key))
    }
    fn task_reply_busy(&self) -> bool {
        matches!(&self.screen, Screen::Connected(p) if p.active_turn.is_some())
    }

    fn is_busy(&self) -> bool {
        matches!(&self.screen, Screen::Connected(p) if p.active_turn.is_some() || !p.active_implementations.is_empty())
    }

    fn conversation_busy(&self) -> bool {
        matches!(&self.screen, Screen::Connected(p) if p.active_turn.is_some())
    }
    fn task_progress(&self, ticket: &str) -> Option<&crate::harness::LiveProgress> {
        match &self.screen {
            Screen::Connected(p) => p.activity.tasks.get(ticket),
            _ => None,
        }
    }
    fn max_parallel_tasks(&self) -> usize {
        match &self.screen {
            Screen::Connected(p) => p.queue.max_parallel.clamp(1, 8),
            _ => 3,
        }
    }
    fn active_task_count(&self) -> usize {
        match &self.screen {
            Screen::Connected(p) => p.active_implementations.len(),
            _ => 0,
        }
    }
    fn live_progress(&self) -> Option<&crate::harness::LiveProgress> {
        match &self.screen {
            Screen::Connected(p)
                if p.task_chats.active.is_none()
                    && (p.active_turn.is_some() || p.activity.manager.is_some()) =>
            {
                Some(&p.live_progress)
            }
            _ => None,
        }
    }

    fn task_offer(&self) -> Option<&crate::core::workflow::InterviewBrief> {
        match &self.screen {
            Screen::Connected(p)
                if p.active_turn.is_none()
                    && p.active_implementations.is_empty()
                    && !has_current_task_batch(p)
                    && p.state.workflow.ready(p.state.planning_contract()) =>
            {
                p.state.workflow.brief.as_ref()
            }
            _ => None,
        }
    }

    fn implementation_offer(&self) -> bool {
        matches!(&self.screen, Screen::Connected(p)
            if p.active_turn.is_none()
                && p.active_implementations.is_empty()
                && has_current_task_batch(p)
                && matches!(crate::core::implementation_queue::next_ticket(
                    &p.task_documents, &p.implementation_states), Ok(Some(_))))
    }

    fn implementation_state(
        &self,
        ticket: &str,
    ) -> Option<&crate::core::implementation::Implementation> {
        match &self.screen {
            Screen::Connected(p) => p.implementation_states.get(ticket),
            _ => None,
        }
    }
    fn implementation_failure(&self, ticket: &str) -> Option<&str> {
        match &self.screen {
            Screen::Connected(p) => p
                .queue
                .blocked
                .get(ticket)
                .map(|failure| failure.message.as_str()),
            _ => None,
        }
    }
    fn task_detail_view(&mut self, ticket: &str) -> Option<crate::ui::task_detail::ViewModel> {
        let (
            implementation,
            implementation_active,
            queued_failure,
            failure_disposition,
            messages,
            progress,
            conversation_active,
            conversation_error,
            draft,
            auto_build,
            can_start,
        ) = match &self.screen {
            Screen::Connected(project) => (
                project.implementation_states.get(ticket).cloned(),
                project.active_implementations.contains_key(ticket),
                project
                    .queue
                    .blocked
                    .get(ticket)
                    .map(|failure| failure.message.clone()),
                project
                    .queue
                    .blocked
                    .get(ticket)
                    .map(|failure| failure.recovery),
                project
                    .task_chats
                    .messages
                    .get(ticket)
                    .cloned()
                    .unwrap_or_default(),
                project.activity.tasks.get(ticket).cloned(),
                project.task_turns.contains_key(ticket),
                project.task_chats.error.clone(),
                project
                    .task_chats
                    .drafts
                    .get(ticket)
                    .cloned()
                    .unwrap_or_default(),
                project.queue.auto_build,
                project.active_turn.is_none()
                    && project.active_implementations.len()
                        < project.queue.max_parallel.clamp(1, 8),
            ),
            Screen::Welcome => return None,
        };
        let failure = queued_failure.or_else(|| {
            implementation
                .as_ref()
                .filter(|record| {
                    record.status == crate::core::implementation::ImplementationStatus::Blocked
                })
                .map(|record| record.detail.clone())
        });
        let attention = failure
            .as_deref()
            .and_then(|detail| self.attention_view(ticket, detail));
        let base = crate::core::implementation::board_column(
            implementation.as_ref(),
            implementation_active,
        );
        let board_column = if !implementation_active && failure.is_some() {
            3
        } else if implementation_active {
            base
        } else {
            crate::ui::task_chat::board_column(base, &messages, conversation_active)
        };
        Some(crate::ui::task_detail::ViewModel {
            implementation,
            implementation_active,
            failure,
            failure_disposition,
            attention,
            messages,
            progress,
            conversation_active,
            conversation_error,
            board_column,
            can_start,
            auto_build,
            draft,
            activity_samples: self.activity_samples(Some(ticket)),
            activity_active: self.activity_active(ticket),
        })
    }
    fn dispatch(&mut self, command: crate::ui::ApplicationCommand) {
        self.dispatch_ui_command(command);
    }

    fn implementation_active(&self, ticket: &str) -> bool {
        matches!(&self.screen, Screen::Connected(p) if p.active_implementations.contains_key(ticket))
    }
    fn auto_plan(&self) -> bool {
        matches!(&self.screen, Screen::Connected(p) if p.queue.auto_plan)
    }
    fn auto_build(&self) -> bool {
        matches!(&self.screen, Screen::Connected(p) if p.queue.auto_build)
    }
    fn auto_publish(&self) -> bool {
        matches!(&self.screen, Screen::Connected(p) if p.queue.auto_publish)
    }
    fn require_independent_checks(&self) -> bool {
        matches!(&self.screen, Screen::Connected(project) if project.queue.require_independent_checks)
    }
    fn queue_status(&self) -> &str {
        match &self.screen {
            Screen::Connected(p) if !p.queue.last_error.is_empty() => &p.queue.last_error,
            Screen::Connected(p) if p.queue.running => "Automatic build queue is running",
            _ => "",
        }
    }
    fn planning_board(&self) -> crate::ui::planning_board::ViewModel {
        match &self.screen {
            Screen::Connected(p) => {
                let mut planning_items = p
                    .state
                    .items
                    .iter()
                    .chain(&self.synth)
                    .chain(&p.state.resolved_items)
                    .cloned()
                    .collect::<Vec<_>>();
                planning_items.sort_by_key(|item| (item.priority.rank(), item.id.clone()));
                planning_items.dedup_by(|a, b| a.id == b.id);
                let eligible_item_ids = planning_items
                    .iter()
                    .filter(|item| {
                        crate::core::routing::eligible_items(
                            std::slice::from_ref(item),
                            &self.cached_user,
                            &p.state.config.stakeholders,
                        )
                        .len()
                            == 1
                    })
                    .map(|item| item.id.clone())
                    .collect();
                crate::ui::planning_board::ViewModel {
                    task_documents: p.task_documents.clone(),
                    planning_work: crate::core::planning_work::cards(&p.state, &p.planning_work),
                    planning_items,
                    eligible_item_ids,
                    archived: p.archived_tasks.clone(),
                }
            }
            _ => crate::ui::planning_board::ViewModel::default(),
        }
    }

    fn next_question_id(&self) -> Option<&str> {
        match &self.screen {
            Screen::Connected(p) => p.next_question_id.as_deref(),
            Screen::Welcome => None,
        }
    }

    fn spec_text(&self) -> &str {
        match &self.screen {
            Screen::Connected(p) => p
                .live_progress
                .specification
                .as_deref()
                .or(p.state.spec_text.as_deref())
                .unwrap_or(NO_SPEC_PLACEHOLDER),
            Screen::Welcome => "",
        }
    }

    fn active_features(&self) -> Vec<(&str, &str)> {
        match &self.screen {
            Screen::Connected(p) => p
                .state
                .active_features
                .iter()
                .map(|(id, body)| (id.as_str(), body.as_str()))
                .collect(),
            Screen::Welcome => Vec::new(),
        }
    }
    fn feature_approved(&self, id: &str) -> bool {
        match &self.screen {
            Screen::Connected(p) => {
                crate::core::workflow::feature_approved(&p.state.repo_root, &p.state.workflow, id)
            }
            Screen::Welcome => false,
        }
    }
    fn feature_actions(
        &self,
        conversation: Option<&str>,
    ) -> Vec<crate::ui::feature_approval::Action> {
        self.available_feature_actions(conversation)
    }
    fn toasts(&mut self) -> &mut ToastQueue {
        &mut self.toasts
    }
}

// --------------------------------------------------------------------- render
impl App for PacketApp {
    fn logic(&mut self, ctx: &egui::Context, _frame: &mut Frame) {
        let dt = ctx.input(|i| i.stable_dt).clamp(0.0, 0.5);
        self.tick(dt, ctx);
    }

    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut Frame) {
        ui.ctx().set_visuals(crate::ui::theme::packet_visuals());
        ui.ctx().style_mut_of(egui::Theme::Dark, |style| {
            style
                .text_styles
                .insert(egui::TextStyle::Body, egui::FontId::proportional(15.0));
            style
                .text_styles
                .insert(egui::TextStyle::Button, egui::FontId::proportional(13.0));
            style.spacing.item_spacing = egui::vec2(8.0, 8.0);
            style.spacing.button_padding = egui::vec2(12.0, 7.0);
        });

        match &mut self.screen {
            Screen::Welcome => {
                let slot = std::cell::RefCell::new(false);
                let browse_slot = std::cell::RefCell::new(false);
                let clone_slot = std::cell::RefCell::new(false);
                // In-flight badge for the card: ("github.com/{o}/{r}", repo).
                let cloning = self
                    .clone_job
                    .as_ref()
                    .map(|j| (j.url_display.as_str(), j.repo.as_str()));
                egui::CentralPanel::default().show(ui, |ui| {
                    ui.vertical_centered(|ui| {
                        ui.add_space((ui.available_height() * 0.30).max(20.0));
                        crate::ui::theme::card_frame().show(ui, |ui| {
                            *slot.borrow_mut() = welcome::paint(
                                ui,
                                &mut self.conn_path,
                                &mut self.conn_github,
                                self.conn_error.as_deref(),
                                &mut browse_slot.borrow_mut(),
                                &mut clone_slot.borrow_mut(),
                                cloning,
                                None,
                                None,
                            );
                        });
                    });
                });
                if *slot.borrow() {
                    self.submit_connect();
                } else if *clone_slot.borrow() {
                    self.begin_clone_from_field();
                } else if *browse_slot.borrow() {
                    // Start the native folder chooser at the current field's
                    // directory, or its parent when the field is a file path.
                    let current = std::path::PathBuf::from(self.conn_path.trim());
                    let start = if current.is_dir() {
                        Some(current.clone())
                    } else {
                        current
                            .parent()
                            .filter(|parent| parent.is_dir())
                            .map(std::path::Path::to_path_buf)
                    }
                    .or_else(|| std::env::current_dir().ok());
                    let mut picker = rfd::FileDialog::new().set_title("Choose a workspace folder");
                    if let Some(start) = start {
                        picker = picker.set_directory(start);
                    }
                    // Cancel leaves the field untouched. Selecting a folder
                    // only fills the field; Open workspace remains the
                    // single place that attempts a connection.
                    if let Some(path) = picker.pick_folder() {
                        let path = path.canonicalize().unwrap_or(path);
                        self.conn_path = path.to_string_lossy().into_owned();
                    }
                }
            }
            Screen::Connected(_) => {
                let surface: &mut dyn Surface = self;
                crate::ui::layout::paint(ui, surface);
            }
        }

        if let Some(dialog) = self.dialog.take() {
            self.render_dialog(ui, dialog);
        }

        self.toasts.show(ui.ctx());
    }
}

impl PacketApp {
    /// Paint a modal with the dialog moved OUT of `self` (clean borrows);
    /// the dialog is put back unless the user closed or completed it.
    fn render_dialog(&mut self, ui: &mut egui::Ui, dialog: Dialog) {
        match dialog {
            Dialog::Import(mut d) => {
                let save_slot = std::cell::RefCell::new(false);
                let close_slot = std::cell::RefCell::new(false);
                let closed = crate::ui::overlays::show_modal(
                    ui,
                    true,
                    "Import reference documents",
                    560.0,
                    |ui| {
                        let (save, close) = dialogs::paint_import_card(ui, &mut d);
                        *save_slot.borrow_mut() = save;
                        *close_slot.borrow_mut() = close;
                    },
                );
                if *save_slot.borrow() {
                    self.perform_import(&mut d);
                }
                let positive = d.feedback.as_ref().is_some_and(|(ok, _)| *ok);
                if !closed && !*close_slot.borrow() && !positive {
                    self.dialog = Some(Dialog::Import(d));
                }
            }
            Dialog::Settings(mut d) => {
                let save_slot = std::cell::RefCell::new(false);
                let close_slot = std::cell::RefCell::new(false);
                let closed = crate::ui::overlays::show_modal(
                    ui,
                    true,
                    "Stakeholders & ownership",
                    660.0,
                    |ui| {
                        let (save, close) = dialogs::paint_settings_card(ui, &mut d);
                        *save_slot.borrow_mut() = save;
                        *close_slot.borrow_mut() = close;
                    },
                );
                if *save_slot.borrow() {
                    self.perform_settings(&mut d);
                }
                let positive = d.feedback.as_ref().is_some_and(|(ok, _)| *ok);
                if !closed && !*close_slot.borrow() && !positive {
                    self.dialog = Some(Dialog::Settings(d));
                }
            }
            Dialog::Mcp(mut d) => {
                let save_slot = std::cell::RefCell::new(false);
                let close_slot = std::cell::RefCell::new(false);
                let closed = crate::ui::overlays::show_modal(
                    ui,
                    true,
                    "MCP server configuration",
                    660.0,
                    |ui| {
                        let (save, close) = dialogs::paint_mcp_card(ui, &mut d);
                        *save_slot.borrow_mut() = save;
                        *close_slot.borrow_mut() = close;
                    },
                );
                if *save_slot.borrow() {
                    self.perform_mcp(&mut d);
                }
                // Keep-open is owned by `perform_mcp` via `d.keep_open`
                // (a malformed save must survive its own successful feedback).
                if !closed && !*close_slot.borrow() && d.keep_open {
                    self.dialog = Some(Dialog::Mcp(d));
                }
            }
            #[cfg(test)]
            Dialog::Browse(mut d) => {
                let choose_slot = std::cell::RefCell::new(false);
                let cancel_slot = std::cell::RefCell::new(false);
                let closed = crate::ui::overlays::show_modal(
                    ui,
                    true,
                    "Choose a workspace folder",
                    560.0,
                    |ui| {
                        let (choose, cancel) = dialogs::paint_browse_card(ui, &mut d);
                        *choose_slot.borrow_mut() = choose;
                        *cancel_slot.borrow_mut() = cancel;
                    },
                );
                // Choosing writes ONLY `conn_path` (an absolute canonical
                // path): no connect attempt, no toast, and `conn_error`
                // keeps describing the last ATTEMPTED connect. The Open
                // button / Enter via submit_connect remains the single
                // connect authority. The defensive Welcome guard mirrors
                // the arms that only push from that screen.
                if *choose_slot.borrow() && matches!(self.screen, Screen::Welcome) {
                    self.conn_path = d.selection().to_string_lossy().into_owned();
                }
                // Standard put-back: reopen unless closed (X/Escape),
                // cancelled, or positively completed (Choose).
                if !closed && !*cancel_slot.borrow() && !*choose_slot.borrow() {
                    self.dialog = Some(Dialog::Browse(d));
                }
            }
        }
    }

    fn perform_import(&mut self, d: &mut DlgImport) {
        let result = match &mut self.screen {
            Screen::Connected(p) => d.apply(p),
            _ => return,
        };
        match result {
            Ok(n) => {
                d.feedback = Some((true, format!("Imported {n} document(s).")));
                if n > 0 {
                    self.dialog = None;
                    self.toasts
                        .success(format!("Imported {n} reference doc(s)"));
                }
            }
            Err(e) => d.feedback = Some((false, e.detail())),
        }
    }

    fn perform_settings(&mut self, d: &mut DlgSettings) {
        let result = match &mut self.screen {
            Screen::Connected(p) => d.apply(p),
            _ => return,
        };
        match result {
            Ok(sha) => {
                self.dialog = None;
                self.toasts.success(format!("Saved · checkpoint {sha}"));
            }
            Err(e) => d.feedback = Some((false, e.detail())),
        }
    }

    /// F-18 outcome table (design-pinned):
    /// * Unchanged → close, neutral feedback, INFO toast (zero churn, NFR-9).
    /// * Write + well-formed → close, success toast with the 7-char SHA.
    /// * Write + MALFORMED → KEEP OPEN with the sticky orange warning; the
    ///   save DID land, so feedback stays positive (consumers sit outside
    ///   the planner — D-16 non-blocking).
    /// * Clear → close, success toast with the checkpoint SHA.
    /// * Err → the disk write may have landed; the checkpoint FAILED. Honest
    ///   accounting: explain that retrying will NOT re-create the commit.
    fn perform_mcp(&mut self, d: &mut DlgMcp) {
        let result = match &mut self.screen {
            Screen::Connected(p) => d.apply(p),
            _ => return,
        };
        // Keep the header's dirty indicator truthful after a disk effect.
        if let Screen::Connected(p) = &mut self.screen {
            p.refresh_git();
        }
        match result {
            Ok(rec) => {
                let sha = rec.short_sha.clone().unwrap_or_default();
                match rec.op {
                    crate::artifacts::mcp_io::McpSaveOp::Unchanged => {
                        d.keep_open = false;
                        d.feedback = Some((true, "Unchanged — no write, no checkpoint.".into()));
                        self.toasts.info("MCP configuration already in sync");
                    }
                    crate::artifacts::mcp_io::McpSaveOp::Write => match rec.malformed {
                        None => {
                            d.keep_open = false;
                            d.feedback = Some((true, format!("Saved · checkpoint {sha}")));
                            self.toasts
                                .success(format!("MCP servers saved · checkpoint {sha}"));
                        }
                        Some(parse_error) => {
                            d.keep_open = true;
                            d.warning = Some(format!(
                                "Not valid JSON: {parse_error} — saved anyway; \
                                 consumers sit outside the planner. Turns \
                                 advertise it verbatim until fixed."
                            ));
                            d.feedback = Some((
                                true,
                                format!("Saved · checkpoint {sha} — kept open, see warning"),
                            ));
                        }
                    },
                    crate::artifacts::mcp_io::McpSaveOp::Clear => {
                        d.keep_open = false;
                        d.feedback = Some((true, format!("Cleared · checkpoint {sha}")));
                        self.toasts
                            .success(format!("MCP servers cleared · checkpoint {sha}"));
                    }
                }
            }
            Err(e) => {
                d.feedback = Some((
                    false,
                    format!(
                        "Disk write may have landed; git checkpoint failed: {} — \
                         retrying Save won't re-create the commit (the file \
                         already matches). Review git state or commit in a later turn.",
                        e.detail()
                    ),
                ));
                // keep_open stays as initialized (true): honest red feedback,
                // operator stays in the card to react.
            }
        }
    }
}

const NO_SPEC_PLACEHOLDER: &str = "# No specification yet

Describe what you are building in the chat. The planner will draft this page for you and keep every subsequent revision under git.";

#[cfg(test)]
mod board_tests {
    use super::*;

    pub(super) fn fixture() -> PacketApp {
        static NEXT_FIXTURE: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
        let chat_slug = std::env::temp_dir()
            .join(format!(
                "packet-board-chat-{}-{}",
                std::process::id(),
                NEXT_FIXTURE.fetch_add(1, std::sync::atomic::Ordering::Relaxed)
            ))
            .to_string_lossy()
            .into_owned();
        let root = std::env::temp_dir().join("packet-board-ui-fixture-nonexistent");
        let docs = ["First task", "Review task", "Merged task"]
            .iter()
            .enumerate()
            .map(|(i, title)| crate::artifacts::task_docs::TaskDocument {
                path: format!(
                    ".kool-ade-packet/planning/tasks/fixture/{:03}-task.md",
                    i + 1
                ),
                title: title.to_string(),
                text: format!("# {title}\n\nUnique story detail {i}"),
                identity: None,
                metadata: None,
                metadata_error: None,
            })
            .collect::<Vec<_>>();
        let mut states = std::collections::BTreeMap::new();
        for (i, pr_state) in [(1, "OPEN"), (2, "MERGED")] {
            let record = crate::core::implementation::Implementation {
                ticket: docs[i].path.clone(),
                task_uid: None,
                ticket_text: docs[i].text.clone(),
                approved_specification: None,
                approved_product_context: None,
                completed_dependency_context: None,
                branch: "packet/fixture".into(),
                base: "main".into(),
                base_commit: "fixture".into(),
                worktree: root.join("worktree"),
                status: if i == 1 {
                    ImplementationStatus::AwaitingReview
                } else {
                    ImplementationStatus::Completed
                },
                detail: String::new(),
                pr_url: Some(format!("https://github.com/fixture/repo/pull/{i}")),
                verified_head: Some("fixture".into()),
                auto_merge: false,
                merged_commit: None,
                pr_state: PullRequestState::parse_legacy(pr_state),
                pr_checked_at: None,
                pr_check_attempted_at: None,
                pr_check_error: None,
                independent_check: None,
                cleanup: Default::default(),
            };
            states.insert(record.ticket.clone(), record);
        }
        PacketApp {
            screen: Screen::Connected(Box::new(Project {
                task_chats: Default::default(),
                activity: Default::default(),
                state: crate::core::state::PlannerState::load(&root).unwrap(),
                chat_slug,
                chat: Vec::new(),
                draft: String::new(),
                queue: Default::default(),
                queue_lock: None,
                active_implementations: Default::default(),
                implementation_states: states,
                pr_refresh: None,
                reconciliation: Default::default(),
                investigation: None,
                investigation_attempted: Default::default(),
                investigation_cooldown_until: None,
                last_pr_refresh: None,
                active_turn: None,
                task_turns: Default::default(),
                task_live: Default::default(),
                planning_work: Default::default(),
                active_planning_work: None,
                live_progress: Default::default(),
                next_question_id: None,
                git: Default::default(),
                task_documents: docs,
                archived_tasks: Default::default(),
            })),
            ..Default::default()
        }
    }

    #[test]
    fn completed_task_shows_cleanup_failure_without_reopening_implementation() {
        let mut app = fixture();
        if let Screen::Connected(p) = &mut app.screen {
            let record = p
                .implementation_states
                .get_mut(".kool-ade-packet/planning/tasks/fixture/003-task.md")
                .unwrap();
            record.cleanup.error = Some("Worktree contains local changes".into());
        }
        let ctx = egui::Context::default();
        frame(&mut app, &ctx, vec![]);
        let output = frame(&mut app, &ctx, vec![]);
        assert!(text_position(&output, "Done · 1").is_some());
        assert!(text_position(&output, "Cleanup needs attention").is_some());
        assert!(text_position(&output, "Worktree contains local changes").is_some());
    }

    #[test]
    fn failed_task_without_saved_state_shows_cause_on_board() {
        let mut app = fixture();
        if let Screen::Connected(p) = &mut app.screen {
            p.queue.blocked.insert(
                p.task_documents[0].path.clone(),
                crate::core::implementation::Failure::other("No space left on device"),
            );
        }
        let ctx = egui::Context::default();
        frame(&mut app, &ctx, vec![]);
        let output = frame(&mut app, &ctx, vec![]);
        assert!(text_position(&output, "Needs attention · 1").is_some());
        assert!(text_position(&output, "No space left on device").is_some());
        assert!(text_position(&output, "Failure details").is_some());
    }

    #[test]
    fn task_details_show_full_state_and_inline_reply() {
        let mut app = fixture();
        let key = ".kool-ade-packet/planning/tasks/fixture/001-task.md";
        if let Screen::Connected(p) = &mut app.screen {
            p.queue.blocked.insert(p.task_documents[0].path.clone(),
                crate::core::implementation::Failure::new(
                    crate::core::implementation::FailureKind::ExternalPrerequisite,
                    crate::core::implementation::RecoveryDisposition::UserAction,
                    "## Waiting for user action\n\nThe published history conflicts with the gate.\n\n### Next action(s)\n\n- Adjudicator: approve the corrected footprint.\n- Operator: record the display demonstration.\n\nFull report: saved-report.json",
                ));
        }
        app.attention_fixture.insert(
            key.into(),
            crate::core::attention::Brief {
                problem: "The published history conflicts with the required file list.".into(),
                recommendation: None,
                options: Vec::new(),
                steps: vec![crate::core::attention::HumanStep {
                    owner: "Operator".into(),
                    action: "Record the display demonstration.".into(),
                }],
                after: "Resume once the required review is complete.".into(),
            },
        );
        let ctx = egui::Context::default();
        frame(&mut app, &ctx, vec![]);
        let output = click_text(&mut app, &ctx, "First task");
        for label in [
            "CURRENT STATE",
            "YOUR NEXT STEP",
            "Activity",
            "Resume after action",
            "Reply to this task",
            "Send response",
        ] {
            assert!(text_position(&output, label).is_some(), "missing {label}");
        }
        assert!(
            text_position(
                &output,
                "The published history conflicts with the required file list."
            )
            .is_some()
        );
        assert!(text_position(&output, "Operator: Record the display demonstration.").is_some());
        assert!(text_position(&output, "Full blocker report").is_some());
        assert!(text_position(&output, "Full report: saved-report.json").is_some());
        assert!(text_position(&output, "Copy full report").is_some());
        assert!(output.shapes.iter().any(|shape| matches!(&shape.shape,
            egui::Shape::Text(text) if text.galley.text().contains("Full report: saved-report.json"))),
            "the complete failure text must be rendered, not shortened to a summary");
    }

    #[test]
    fn task_details_show_independent_check_result_separately_from_packet_verification() {
        let mut app = fixture();
        let key = ".kool-ade-packet/planning/tasks/fixture/001-task.md";
        if let Screen::Connected(project) = &mut app.screen {
            project.implementation_states.insert(
                key.into(),
                crate::core::implementation::Implementation {
                    ticket: key.into(),
                    task_uid: None,
                    ticket_text: "# First task".into(),
                    approved_specification: None,
                    approved_product_context: None,
                    completed_dependency_context: None,
                    branch: "packet/fixture".into(),
                    base: "main".into(),
                    base_commit: "fixture-base".into(),
                    worktree: std::path::PathBuf::from("/tmp/packet-fixture"),
                    status: crate::core::implementation::ImplementationStatus::Completed,
                    detail: "Locally verified.".into(),
                    pr_url: None,
                    verified_head: Some("0123456789abcdef".into()),
                    auto_merge: false,
                    merged_commit: Some("0123456789abcdef".into()),
                    pr_state: None,
                    pr_checked_at: None,
                    pr_check_attempted_at: None,
                    pr_check_error: None,
                    independent_check: Some(crate::core::implementation::IndependentCheck {
                        provider: "GitHub Actions".into(),
                        commit: "0123456789abcdef".into(),
                        candidate_ref: "refs/heads/packet/checks/task/0123456789abcdef".into(),
                        status: crate::core::implementation::IndependentCheckStatus::Passed,
                        checked_at: None,
                        detail: Some("All project workflows passed for this exact commit.".into()),
                    }),
                    cleanup: Default::default(),
                },
            );
        }
        let ctx = egui::Context::default();
        frame(&mut app, &ctx, vec![]);
        let output = click_text(&mut app, &ctx, "First task");
        for expected in [
            "Completed",
            "GitHub Actions · Passed",
            "commit 0123456789ab",
            "All project workflows passed for this exact commit.",
        ] {
            assert!(
                text_position(&output, expected).is_some(),
                "missing {expected}"
            );
        }
    }

    #[test]
    fn task_details_offer_open_options_in_the_reply_box() {
        let mut app = fixture();
        let key = ".kool-ade-packet/planning/tasks/fixture/001-task.md";
        if let Screen::Connected(p) = &mut app.screen {
            p.task_chats.messages.insert(key.into(), vec![ChatMessage::new(
                ChatRole::Agent,
                "Ready.\n\n---\n- Which approach?\n- Yes, use the existing adapter.\n- No, replace the adapter.",
                Some(key.into()),
            )]);
        }
        let ctx = egui::Context::default();
        frame(&mut app, &ctx, vec![]);
        let output = click_text(&mut app, &ctx, "First task");
        for label in [
            "Which approach?",
            "Choose an option",
            "Yes, use the existing adapter.",
            "No, replace the adapter.",
            "Send response",
        ] {
            assert!(text_position(&output, label).is_some(), "missing {label}");
        }
        click_text(&mut app, &ctx, "Yes, use the existing adapter.");
        if let Screen::Connected(p) = &app.screen {
            assert_eq!(
                p.task_chats.drafts.get(key).map(String::as_str),
                Some("Yes, use the existing adapter.")
            );
        }
    }

    #[test]
    fn generated_attention_brief_explains_an_unseen_blocker_and_sends_its_choice() {
        let mut app = fixture();
        let key = ".kool-ade-packet/planning/tasks/fixture/001-task.md";
        if let Screen::Connected(p) = &mut app.screen {
            p.queue.blocked.insert(key.into(), crate::core::implementation::Failure::other("## Waiting for user action\n\nA provider quota stopped the job.\n\n### Next action(s)\n\n- Account owner: choose (a) wait or (b) request more capacity.\n\nFull report: report.json"));
        }
        app.attention_fixture.insert(key.into(), crate::core::attention::Brief {
            problem: "The provider has reached its daily request limit, so the job cannot continue today.".into(),
            recommendation: Some(crate::core::attention::Recommendation {
                option_id: "a".into(),
                rationale: "Waiting avoids account changes and extra charges; the report says capacity returns tomorrow.".into(),
            }),
            options: vec![
                crate::core::attention::OptionBrief { id: "a".into(), label: "Wait for reset".into(),
                    meaning: "Use the existing quota after it refreshes.".into(),
                    consequence: "There is no account change, but the task remains paused until tomorrow.".into(), source_evidence: None },
                crate::core::attention::OptionBrief { id: "b".into(), label: "Request higher quota".into(),
                    meaning: "Ask the provider to raise the account limit.".into(),
                    consequence: "This may require account approval or added cost; the task remains paused until capacity is granted.".into(), source_evidence: None },
            ],
            steps: vec![crate::core::attention::HumanStep {
                owner: "Account owner".into(), action: "Choose how to get more capacity.".into(),
            }],
            after: "Packet can retry once capacity is available.".into(),
        });
        let ctx = egui::Context::default();
        frame(&mut app, &ctx, vec![]);
        let output = click_text(&mut app, &ctx, "First task");
        for label in [
            "The provider has reached its daily request limit, so the job cannot continue today.",
            "Packet recommends",
            "Wait for reset: Waiting avoids account changes and extra charges; the report says capacity returns tomorrow.",
            "Account owner: Choose how to get more capacity.",
            "Wait for reset",
            "Request higher quota",
            "If chosen: There is no account change, but the task remains paused until tomorrow.",
            "If chosen: This may require account approval or added cost; the task remains paused until capacity is granted.",
            "Packet can retry once capacity is available.",
            "Send decision",
        ] {
            assert!(text_position(&output, label).is_some(), "missing {label}");
        }
        click_text(&mut app, &ctx, "Request higher quota");
        if let Screen::Connected(p) = &app.screen {
            assert_eq!(
                p.task_chats.drafts.get(key).map(String::as_str),
                Some("I choose option (b): Request higher quota.")
            );
        }
        let output = click_text(&mut app, &ctx, "Send decision");
        assert!(text_position(&output, "Decision saved for Packet.").is_some());
        assert!(text_position(&output, "Change decision").is_some());
        if let Screen::Connected(p) = &app.screen {
            assert!(
                p.task_turns.is_empty(),
                "decision should not start a planner turn"
            );
            assert!(p.task_chats.drafts.get(key).is_none_or(String::is_empty));
            assert!(
                p.task_chats.messages[key]
                    .last()
                    .is_some_and(|m| m.role == ChatRole::User && m.text.contains("option (b)"))
            );
            let mut saved = crate::persistence::task_chats::TaskChats::default();
            saved.ensure_loaded(&p.chat_slug);
            assert!(
                saved.messages[key]
                    .last()
                    .is_some_and(|m| m.text.contains("option (b)"))
            );
        }
    }

    #[test]
    fn board_shows_graph_only_for_running_cards_and_sums_all_sources() {
        let mut app = fixture();
        if let Screen::Connected(p) = &mut app.screen {
            for (key, count) in [
                (".kool-ade-packet/planning/tasks/fixture/001-task.md", 2),
                ("another-task", 3),
            ] {
                p.activity
                    .tasks
                    .entry(key.into())
                    .or_default()
                    .telemetry
                    .samples = vec![(100, count)];
            }
            p.activity
                .conversations
                .entry("__main".into())
                .or_default()
                .telemetry
                .samples = vec![(100, 5), (101, 1)];
            p.activity
                .conversations
                .entry(".kool-ade-packet/planning/tasks/fixture/001-task.md".into())
                .or_default()
                .telemetry
                .samples = vec![(100, 7)];
            p.active_implementations.insert(
                ".kool-ade-packet/planning/tasks/fixture/001-task.md".into(),
                crate::core::implementation::Controller::idle_fixture(),
            );
        }
        assert_eq!(app.activity_samples(None), vec![(100, 17), (101, 1)]);
        assert_eq!(
            app.activity_samples(Some(".kool-ade-packet/planning/tasks/fixture/001-task.md")),
            vec![(100, 9)]
        );
        let ctx = egui::Context::default();
        frame(&mut app, &ctx, vec![]);
        let output = frame(&mut app, &ctx, vec![]);
        assert!(text_position(&output, "All activity").is_some());
        let red_lines = output.shapes.iter().filter(|shape| matches!(&shape.shape, egui::Shape::Path(path)
            if path.points.len() == 60 && path.stroke.color == egui::epaint::ColorMode::Solid(crate::ui::theme::DANGER))).count();
        assert_eq!(
            red_lines, 2,
            "the overview and running card graphs are visible"
        );
        // Starting another run must not erase the project's observed history.
        if let Screen::Connected(p) = &mut app.screen {
            p.activity.ensure_overall();
            p.activity.tasks.clear();
            p.activity.conversations.clear();
        }
        assert_eq!(app.activity_samples(None), vec![(100, 17), (101, 1)]);
    }

    #[test]
    fn compact_task_cards_only_show_inputs_for_pending_answers() {
        let mut app = fixture();
        let ctx = egui::Context::default();
        frame(&mut app, &ctx, vec![]);
        let output = frame(&mut app, &ctx, vec![]);
        assert!(text_position(&output, "Add context").is_none());
        assert!(text_position(&output, "Your answer…").is_none());
        assert!(text_position(&output, "Send answer").is_none());
        assert!(text_position(&output, "Open conversation").is_some());
    }

    #[test]
    fn done_task_can_be_archived_off_the_board() {
        let mut app = fixture();
        let ctx = egui::Context::default();
        frame(&mut app, &ctx, vec![]);
        let output = frame(&mut app, &ctx, vec![]);
        assert!(text_position(&output, "Merged task").is_some());
        assert!(text_position(&output, "Archive").is_some());
        let output = click_text(&mut app, &ctx, "Archive");
        assert!(text_position(&output, "Merged task").is_none());
        let Screen::Connected(project) = &app.screen else {
            panic!("disconnected")
        };
        let ticket = ".kool-ade-packet/planning/tasks/fixture/003-task.md";
        assert!(project.archived_tasks.contains(ticket));
        assert!(crate::persistence::archived_tasks::load(&project.chat_slug).contains(ticket));
    }

    pub(super) fn frame(
        app: &mut PacketApp,
        ctx: &egui::Context,
        events: Vec<egui::Event>,
    ) -> egui::FullOutput {
        frame_at(app, ctx, events, egui::vec2(1800.0, 900.0))
    }

    fn frame_at(
        app: &mut PacketApp,
        ctx: &egui::Context,
        events: Vec<egui::Event>,
        size: egui::Vec2,
    ) -> egui::FullOutput {
        for theme in [egui::Theme::Dark, egui::Theme::Light] {
            ctx.style_mut_of(theme, |style| style.animation_time = 0.0);
        }
        let mut output = ctx.run_ui(
            egui::RawInput {
                screen_rect: Some(egui::Rect::from_min_size(egui::Pos2::ZERO, size)),
                events,
                ..Default::default()
            },
            |ui| crate::ui::layout::paint(ui, app),
        );
        output.textures_delta.clear();
        output
    }
    pub(super) fn text_position(output: &egui::FullOutput, needle: &str) -> Option<egui::Pos2> {
        output.shapes.iter().find_map(|shape| {
            if let egui::Shape::Text(text) = &shape.shape
                && text.galley.text() == needle
            {
                return Some(text.pos + text.galley.mesh_bounds.center().to_vec2());
            }
            None
        })
    }
    pub(super) fn text_contains(output: &egui::FullOutput, needle: &str) -> bool {
        output.shapes.iter().any(|shape| {
            matches!(
                &shape.shape,
                egui::Shape::Text(text) if text.galley.text().contains(needle)
            )
        })
    }

    pub(super) fn click_text(
        app: &mut PacketApp,
        ctx: &egui::Context,
        label: &str,
    ) -> egui::FullOutput {
        click_text_at(app, ctx, label, egui::vec2(1800.0, 900.0))
    }

    fn click_text_at(
        app: &mut PacketApp,
        ctx: &egui::Context,
        label: &str,
        size: egui::Vec2,
    ) -> egui::FullOutput {
        let output = frame_at(app, ctx, vec![], size);
        let click =
            text_position(&output, label).unwrap_or_else(|| panic!("missing clickable {label}"));
        frame_at(
            app,
            ctx,
            vec![
                egui::Event::PointerMoved(click),
                egui::Event::PointerButton {
                    pos: click,
                    button: egui::PointerButton::Primary,
                    pressed: true,
                    modifiers: Default::default(),
                },
            ],
            size,
        );
        frame_at(
            app,
            ctx,
            vec![egui::Event::PointerButton {
                pos: click,
                button: egui::PointerButton::Primary,
                pressed: false,
                modifiers: Default::default(),
            }],
            size,
        );
        frame_at(app, ctx, vec![], size)
    }

    #[test]
    fn narrow_workspace_keeps_chat_and_board_visible() {
        let mut app = fixture();
        let ctx = egui::Context::default();
        let size = egui::vec2(360.0, 480.0);
        frame_at(&mut app, &ctx, vec![], size);
        let output = frame_at(&mut app, &ctx, vec![], size);
        let chat = text_position(&output, "Main Chat").unwrap();
        let board = text_position(&output, "Board  3").unwrap();
        let send = text_position(&output, "Ctrl + Enter to send").unwrap();
        assert!(chat.y < send.y && send.y < board.y);
        assert!(board.x < size.x && board.y < size.y);
        assert_eq!(output.viewport_output.len(), 1);
    }

    #[test]
    fn long_drafts_keep_send_controls_inside_chat_panel() {
        for size in [egui::vec2(360.0, 480.0), egui::vec2(1480.0, 900.0)] {
            let mut app = fixture();
            let ctx = egui::Context::default();
            let draft = "A long wrapped draft with several words on every line.\n".repeat(150);
            *app.chat_draft() = draft.clone();
            for _ in 0..3 {
                frame_at(&mut app, &ctx, vec![], size);
            }
            let output = frame_at(&mut app, &ctx, vec![], size);
            let send = text_position(&output, "Ctrl + Enter to send")
                .expect("send control remains visible");
            assert!(
                send.y < size.y - 20.0,
                "send at {send:?}, viewport {size:?}"
            );
            if size.x < 960.0 {
                let board = text_position(&output, "Board  3").unwrap();
                assert!(send.y < board.y, "composer must stay above board");
            }
            assert_eq!(app.chat_draft(), &draft);
        }
    }

    #[test]
    fn pending_repository_refresh_does_not_block_ui_interactions() {
        let mut app = fixture();
        let ctx = egui::Context::default();
        let (release, wait) = std::sync::mpsc::channel::<()>();
        app.display_refresh = Some(std::thread::spawn(move || {
            let _ = wait.recv();
            panic!("test worker has no snapshot")
        }));
        app.last_git_refresh = Instant::now() - Duration::from_secs(10);
        app.tick(0.016, &ctx);
        frame(&mut app, &ctx, vec![]);
        click_text(&mut app, &ctx, "Workspace");
        let output = click_text(&mut app, &ctx, "Settings…");
        assert!(text_position(&output, "Workspace settings").is_some());
        assert!(!app.display_refresh.as_ref().unwrap().is_finished());
        release.send(()).unwrap();
        let _ = app.display_refresh.take().unwrap().join();
    }

    /// Turns in `execute` suspend until their cancel flag flips: a faithful
    /// stand-in for an in-flight planner turn whose worker must survive the
    /// sibling spawn completely untouched.
    struct HangingTurnHarness;
    impl crate::harness::AiHarness for HangingTurnHarness {
        fn label(&self) -> String {
            "hanging-fixture 0".into()
        }
        fn check_available(&self) -> Result<String, crate::AppError> {
            Ok("present".into())
        }
        fn execute(
            &self,
            request: &crate::harness::PlanningRequest,
        ) -> Result<crate::harness::HarnessOutcome, crate::AppError> {
            while !request.cancel.load(std::sync::atomic::Ordering::SeqCst) {
                std::thread::sleep(Duration::from_millis(10));
            }
            Err(crate::AppError::Other(String::from(
                "fixture turn cancelled",
            )))
        }
    }

    /// All on-screen text, galley-concatenated without separators so a
    /// phrase spanning a toast's soft line wrap still matches as a whole.
    fn canvas_text(output: &egui::FullOutput) -> String {
        output
            .shapes
            .iter()
            .filter_map(|shape| match &shape.shape {
                egui::Shape::Text(text) => Some(text.galley.text().to_string()),
                _ => None,
            })
            .collect()
    }

    /// Paint ONE SETTLED frame with the app's toast layer included — the
    /// test helper paints `layout::paint` directly, bypassing
    /// `PacketApp::ui`, so the same `toasts.show(..)` call is replayed
    /// INSIDE the layout pass, in the same order the native app uses it.
    /// A freshly introduced toast area needs one settle pass before its
    /// content reaches the paint output, so one pass is discarded and the
    /// settled second pass is returned.
    fn frame_toasting(app: &mut PacketApp, ctx: &egui::Context) -> egui::FullOutput {
        let paint_one_pass = |app: &mut PacketApp, ctx: &egui::Context| -> egui::FullOutput {
            for theme in [egui::Theme::Dark, egui::Theme::Light] {
                ctx.style_mut_of(theme, |style| style.animation_time = 0.0);
            }
            let mut output = ctx.run_ui(
                egui::RawInput {
                    screen_rect: Some(egui::Rect::from_min_size(
                        egui::Pos2::ZERO,
                        egui::vec2(1800.0, 900.0),
                    )),
                    events: vec![],
                    ..Default::default()
                },
                |ui| {
                    crate::ui::layout::paint(ui, app);
                    app.toasts().show(ui.ctx());
                },
            );
            output.textures_delta.clear();
            output
        };
        let _ = paint_one_pass(app, ctx);
        paint_one_pass(app, ctx)
    }

    // Mirrors the production pattern deliberately: the UI owns the live turn
    // handle locally, and the assertion needs exactly that Rc's identity.
    #[test]
    fn open_workspace_spawns_a_detached_sibling_without_touching_the_session() {
        let mut app = fixture();
        let ctx = egui::Context::default();
        frame(&mut app, &ctx, vec![]);

        // Benign spawn target (unit-test pick of record: a trivial no-op
        // present on the Linux target hosts).
        const HARMLESS: [&str; 2] = ["/usr/bin/true", "/bin/true"];
        let target = HARMLESS
            .iter()
            .copied()
            .find(|cand| std::path::Path::new(cand).is_file())
            .unwrap_or_else(|| panic!("no trivial no-op utility on this Linux host"));
        app.spawn_target_override = Some(target.into());

        // Fake in-flight work: a real-but-suspended turn plus an active
        // implementation, alongside an unsent composer draft.
        let ticket = ".kool-ade-packet/planning/tasks/fixture/001-task.md".to_string();
        let running = {
            let Screen::Connected(project) = &mut app.screen else {
                panic!("fixture must be connected")
            };
            project.draft = "unsent draft must survive the sibling spawn".to_string();
            project.active_implementations.insert(
                ticket.clone(),
                crate::core::implementation::Controller::idle_fixture(),
            );
            std::rc::Rc::new(TurnController::start(
                crate::core::turn::TurnInputs {
                    state: project.state.clone(),
                    user_message: "Please continue".into(),
                    recent_chat: Vec::new(),
                    purpose: crate::core::workflow::TurnPurpose::Interview,
                    comparison_feature: None,
                },
                Box::new(HangingTurnHarness),
            ))
        };
        {
            let Screen::Connected(project) = &mut app.screen else {
                panic!("fixture must be connected")
            };
            project.active_turn = Some(running.clone());
            project.live_progress = crate::harness::LiveProgress {
                activity: Some("Planning…".into()),
                ..Default::default()
            };
        }
        let draft_before = app.chat_draft().clone();

        // Click 1: Workspace → 'Open workspace'.
        click_text(&mut app, &ctx, "Workspace");
        click_text(&mut app, &ctx, "Open workspace");
        let output = frame_toasting(&mut app, &ctx);
        let painted = canvas_text(&output);
        assert!(
            painted.contains("Opening a new") && painted.contains("Packet window"),
            "success toast expected, saw: {}",
            painted.chars().take(400).collect::<String>()
        );
        assert!(
            !painted.contains("Turn aborted"),
            "no 'Turn aborted' toast allowed"
        );
        assert!(
            matches!(app.screen, Screen::Connected(_)),
            "the invoking window must stay Connected"
        );
        assert_eq!(
            app.chat_draft(),
            &draft_before,
            "composer draft must be untouched"
        );
        {
            let Screen::Connected(project) = &app.screen else {
                panic!("fixture must be connected")
            };
            assert!(
                std::rc::Rc::ptr_eq(
                    &running,
                    project.active_turn.as_ref().unwrap_or_else(|| {
                        panic!("the in-flight turn must still be registered")
                    })
                ),
                "same controller still registered"
            );
            assert!(
                !running.cancel_requested(),
                "no cancel request may reach the turn"
            );
            assert!(
                !project
                    .active_implementations
                    .get(&ticket)
                    .unwrap()
                    .cancellation_requested(),
                "no cancel request may reach the implementations"
            );
        }

        // Click 2: repeat invocation spawns again with no shared-state
        // collision — the parent never waits on either Child.
        click_text(&mut app, &ctx, "Workspace");
        click_text(&mut app, &ctx, "Open workspace");
        let repainted = canvas_text(&frame_toasting(&mut app, &ctx));
        assert!(repainted.contains("Opening a new") && repainted.contains("Packet window"));
        assert!(
            !running.cancel_requested(),
            "repeat click must not cancel either"
        );
        assert!(matches!(app.screen, Screen::Connected(_)));
        assert_eq!(app.chat_draft(), &draft_before);
    }

    #[test]
    fn open_workspace_spawn_failure_warns_with_path_and_leaves_the_session_usable() {
        let mut app = fixture();
        let ctx = egui::Context::default();
        frame(&mut app, &ctx, vec![]);
        if let Screen::Connected(project) = &mut app.screen {
            project.draft = "draft survives a failed spawn".to_string();
        }
        let missing = std::env::temp_dir().join(format!("packet-sib-{}", std::process::id()));
        assert!(!missing.exists(), "test pre-condition");
        // Single word-group (no interior spaces), so it survives the toast's
        // soft line wrapping intact.
        let token = format!("packet-sib-{}", std::process::id());
        app.spawn_target_override = Some(missing);

        click_text(&mut app, &ctx, "Workspace");
        click_text(&mut app, &ctx, "Open workspace");
        let output = frame_toasting(&mut app, &ctx);
        let painted = canvas_text(&output);
        assert!(
            painted.contains(&token),
            "warning toast must embed the failing binary path"
        );
        assert!(
            !(painted.contains("Opening a new") && painted.contains("Packet window")),
            "no success toast on failure"
        );
        assert!(
            matches!(app.screen, Screen::Connected(_)),
            "screen stays Connected"
        );
        assert_eq!(app.chat_draft(), "draft survives a failed spawn");

        // Retry: the same graceful failure repeats (repeat-request stability,
        // no zombie half-interaction).
        click_text(&mut app, &ctx, "Workspace");
        click_text(&mut app, &ctx, "Open workspace");
        let output = frame_toasting(&mut app, &ctx);
        let painted = canvas_text(&output);
        assert!(
            painted.contains(&token),
            "retry warning must embed the path again"
        );
        assert!(
            !(painted.contains("Opening a new") && painted.contains("Packet window")),
            "no success toast on failure retry"
        );
        assert!(matches!(app.screen, Screen::Connected(_)));
        assert_eq!(app.chat_draft(), "draft survives a failed spawn");
    }

    #[test]
    fn settings_controls_open_in_modal_from_workspace_menu() {
        let mut app = fixture();
        let ctx = egui::Context::default();
        frame(&mut app, &ctx, vec![]);
        let output = frame(&mut app, &ctx, vec![]);
        assert!(text_position(&output, "Build approved changes automatically").is_none());
        assert!(text_position(&output, "Plan automatically").is_none());
        assert!(text_position(&output, "Publish verified changes automatically").is_none());
        assert!(
            text_position(&output, "All activity").unwrap().y
                < text_position(&output, "To do · 1").unwrap().y
        );
        assert!(text_position(&output, "Status").is_none());
        click_text(&mut app, &ctx, "Workspace");
        let output = click_text(&mut app, &ctx, "Settings…");
        assert!(text_position(&output, "Workspace settings").is_some());
        assert!(text_position(&output, "Build approved changes automatically").is_some());
        assert!(text_position(&output, "Plan automatically").is_some());
        assert!(text_position(&output, "Publish verified changes automatically").is_some());
        assert!(text_position(&output, "Wait for project checks before publishing").is_some());
        assert!(
            text_position(
                &output,
                "Saved for this project on this device, across its Packet windows."
            )
            .is_some()
        );
        assert!(text_position(
            &output,
            "Packet checks its work locally first. When Auto Publish is on, it also waits for the project's separate checks before sharing. If those checks fail or are unavailable, verified work stays on this device. Enabling Auto Publish turns on this check."
        ).is_some());
        let policy_repo = match &app.screen {
            Screen::Connected(project) => project.state.repo_root.clone(),
            Screen::Welcome => unreachable!(),
        };
        if let Screen::Connected(project) = &mut app.screen {
            project.investigation = Some(crate::core::investigation::Controller::idle_fixture(
                "CLR-981",
            ));
        }
        click_text(&mut app, &ctx, "Plan automatically");
        assert!(matches!(&app.screen, Screen::Connected(project)
            if !project.queue.auto_plan
                && project.investigation.as_ref().is_some_and(|run| run.cancellation_requested())));
        assert!(
            !crate::core::implementation_queue::Queue::load(&policy_repo)
                .unwrap()
                .auto_plan
        );
        if let Screen::Connected(project) = &mut app.screen {
            project.investigation = None;
            let mut item = OpenItem::new(
                "CLR-981".into(),
                crate::domain::Priority::Normal,
                crate::domain::ItemKind::Question,
                "General".into(),
                None,
                "Can this be resolved from repository evidence?".into(),
                "Automatic planning should own this item.".into(),
            );
            item.authority = crate::domain::Authority::Agent;
            project.state.items.push(item);
        }
        app.advance_investigation();
        assert!(matches!(&app.screen, Screen::Connected(project)
            if !project.queue.auto_plan && project.investigation.is_none()));
        click_text(&mut app, &ctx, "Plan automatically");
        assert!(
            crate::core::implementation_queue::Queue::load(&policy_repo)
                .unwrap()
                .auto_plan
        );
        click_text(&mut app, &ctx, "Build approved changes automatically");
        assert!(matches!(&app.screen, Screen::Connected(project)
            if !project.queue.auto_build && !project.queue.auto_publish));
        let saved = crate::core::implementation_queue::Queue::load(&policy_repo).unwrap();
        assert!(!saved.auto_build && !saved.auto_publish);
        click_text(&mut app, &ctx, "Publish verified changes automatically");
        assert!(matches!(&app.screen, Screen::Connected(project)
            if !project.queue.auto_build && project.queue.auto_publish
                && project.queue.require_independent_checks));
        let saved = crate::core::implementation_queue::Queue::load(&policy_repo).unwrap();
        assert!(!saved.auto_build && saved.auto_publish && saved.require_independent_checks);
        assert!(
            crate::core::implementation_queue::Queue::load(&policy_repo)
                .unwrap()
                .require_independent_checks
        );
        frame(
            &mut app,
            &ctx,
            vec![egui::Event::Key {
                key: egui::Key::Escape,
                physical_key: None,
                pressed: true,
                repeat: false,
                modifiers: Default::default(),
            }],
        );
        let output = frame(&mut app, &ctx, vec![]);
        assert!(text_position(&output, "Workspace settings").is_none());
    }

    #[test]
    fn workspace_repository_menu_uses_the_shared_display_label() {
        let mut app = fixture();
        if let Screen::Connected(project) = &mut app.screen {
            project.state.repositories.repositories[0].display_name =
                Some("Planning repository".into());
        }
        let ctx = egui::Context::default();
        frame(&mut app, &ctx, vec![]);
        frame(&mut app, &ctx, vec![]);
        click_text(&mut app, &ctx, "Workspace");
        let output = click_text(&mut app, &ctx, "Registered repositories");
        assert!(text_contains(
            &output,
            "Open Planning repository in a new window"
        ));
    }

    #[test]
    fn main_chat_is_always_visible_left_of_board() {
        let mut app = fixture();
        let ctx = egui::Context::default();
        *app.chat_draft() = "Keep my project draft".into();
        frame(&mut app, &ctx, vec![]);
        let output = frame(&mut app, &ctx, vec![]);
        let chat = text_position(&output, "Main Chat").unwrap();
        let board = text_position(&output, "Board  3").unwrap();
        assert!(chat.x < board.x);
        assert!(text_position(&output, "Keep my project draft").is_some());
        assert!(text_position(&output, "×").is_none());
        assert_eq!(output.viewport_output.len(), 1);
    }

    #[test]
    fn conversation_tabs_focus_deduplicate_close_and_preserve_drafts() {
        let mut app = fixture();
        let ctx = egui::Context::default();
        let key = ".kool-ade-packet/planning/tasks/fixture/001-task.md";
        *app.chat_draft() = "Keep my project draft".into();
        *app.task_draft(key).unwrap() = "Task draft stays with this item".into();
        if let Screen::Connected(p) = &mut app.screen {
            p.task_chats.messages.insert(
                key.into(),
                vec![ChatMessage::new(
                    ChatRole::Agent,
                    r#"{"assistant_message":"Task-only previous reply"}"#,
                    None,
                )],
            );
        }
        frame(&mut app, &ctx, vec![]);
        click_text(&mut app, &ctx, "Open conversation");
        let output = frame(&mut app, &ctx, vec![]);
        let tabs = || {
            ctx.data_mut(|d| {
                d.get_temp::<crate::ui::layout::ChatTabs>(egui::Id::new("packet_chat_tabs"))
            })
            .unwrap()
        };
        assert_eq!(tabs().keys, vec![key.to_owned()]);
        assert_eq!(tabs().active.as_deref(), Some(key));
        assert!(text_position(&output, "Task draft stays with this item").is_some());
        assert!(text_position(&output, "Task-only previous reply").is_some());
        assert!(text_position(&output, "Keep my project draft").is_none());
        assert_eq!(output.viewport_output.len(), 1);
        let output = click_text(&mut app, &ctx, "Main Chat");
        assert!(tabs().active.is_none());
        assert!(text_position(&output, "Keep my project draft").is_some());
        click_text(&mut app, &ctx, "Open conversation");
        assert_eq!(tabs().keys.len(), 1);
        assert_eq!(tabs().active.as_deref(), Some(key));
        click_text(&mut app, &ctx, "×");
        assert!(tabs().keys.is_empty());
        assert!(tabs().active.is_none());
        assert_eq!(
            app.task_draft(key).unwrap(),
            "Task draft stays with this item"
        );
        assert_eq!(app.chat_draft(), "Keep my project draft");
        click_text(&mut app, &ctx, "Open conversation");
        assert_eq!(tabs().active.as_deref(), Some(key));
    }

    #[test]
    fn narrow_question_modal_keeps_larger_reply_input_and_send_reachable() {
        let mut app = fixture();
        if let Screen::Connected(p) = &mut app.screen {
            p.task_documents.clear();
            p.state.items = vec![OpenItem::new(
                "CLR-050".into(),
                crate::domain::Priority::Normal,
                crate::domain::ItemKind::Question,
                "General".into(),
                Some("All".into()),
                "Which authentication provider?".into(),
                "Choose how users sign in.".into(),
            )];
        }
        let ctx = egui::Context::default();
        let size = egui::vec2(360.0, 480.0);
        frame_at(&mut app, &ctx, vec![], size);
        click_text_at(&mut app, &ctx, "Which authentication provider?", size);
        let output = frame_at(&mut app, &ctx, vec![], size);
        // The last matching control is in the modal, not the board behind it.
        for label in ["Your answer…", "Send answer"] {
            let pos = output
                .shapes
                .iter()
                .rev()
                .find_map(|shape| {
                    if let egui::Shape::Text(text) = &shape.shape {
                        (text.galley.text() == label)
                            .then_some(text.pos + text.galley.mesh_bounds.center().to_vec2())
                    } else {
                        None
                    }
                })
                .unwrap();
            assert!(
                pos.x > 0.0 && pos.x < size.x && pos.y > 0.0 && pos.y < size.y,
                "{label} should be reachable"
            );
        }
    }

    #[test]
    fn narrow_task_workspace_leads_with_action_and_discloses_description() {
        let mut app = fixture();
        app.prepare_task_chat(".kool-ade-packet/planning/tasks/fixture/001-task.md");
        let ctx = egui::Context::default();
        let size = egui::vec2(360.0, 480.0);
        frame_at(&mut app, &ctx, vec![], size);
        click_text_at(&mut app, &ctx, "First task", size);
        let output = frame_at(&mut app, &ctx, vec![], size);
        let action = text_position(&output, "Implement & continue queue").unwrap();
        assert!(action.x > 0.0 && action.x < size.x && action.y > 0.0 && action.y < size.y);
        assert!(text_position(&output, "CURRENT STATE").is_some());
        assert!(text_position(&output, "YOUR NEXT STEP").is_some());
        assert!(text_position(&output, "Activity").is_some());
        assert!(text_position(&output, "Unique story detail 0").is_none());
    }

    #[test]
    fn document_switcher_displays_product_and_multiple_features_without_task_story_tab() {
        let mut app = fixture();
        let feature = |id: &str, title: &str, marker: &str| {
            let markdown = format!("# {id}: {title}\n\n{marker}\n");
            let identified =
                crate::domain::ArtifactIdentity::preserve_markdown(&markdown, None, id, title)
                    .unwrap();
            let identity = crate::domain::ArtifactIdentity::from_markdown(&identified)
                .unwrap()
                .unwrap();
            crate::domain::ChangeMetadata::write_markdown(
                &identified,
                &identity,
                crate::domain::ChangeStatus::Draft,
            )
            .unwrap()
        };
        let first = feature("CHG-001", "First feature", "First proposal marker");
        let second = feature("CHG-002", "Second feature", "Second proposal marker");
        if let Screen::Connected(project) = &mut app.screen {
            project.state.spec_text = Some("# Product\n\nProduct behavior marker".into());
            project.state.active_feature = Some(("CHG-001".into(), first.clone()));
            project.state.active_features =
                vec![("CHG-001".into(), first), ("CHG-002".into(), second)];
        }
        let ctx = egui::Context::default();
        ctx.data_mut(|data| data.insert_temp(egui::Id::new("packet_document_tab"), false));
        frame(&mut app, &ctx, vec![]);
        let product = frame(&mut app, &ctx, vec![]);
        assert!(text_position(&product, "Product behavior marker").is_some());
        assert!(text_position(&product, "Task Stories").is_none());
        assert!(text_position(&product, "First proposal marker").is_none());
        let first = click_text(&mut app, &ctx, "Features  2");
        assert!(text_position(&first, "First proposal marker").is_some());
        assert!(text_position(&first, "Product behavior marker").is_none());
        ctx.data_mut(|data| {
            data.insert_temp(
                egui::Id::new("packet_selected_feature"),
                "CHG-002".to_string(),
            )
        });
        let second = frame(&mut app, &ctx, vec![]);
        assert!(text_position(&second, "Second proposal marker").is_some());
        assert!(text_position(&second, "First proposal marker").is_none());
    }

    #[test]
    fn board_cards_select_story_details_and_open_the_correct_pr() {
        let mut app = fixture();
        let ctx = egui::Context::default();
        ctx.data_mut(|d| d.insert_temp(egui::Id::new("packet_document_tab"), true));
        frame(&mut app, &ctx, vec![]);
        let output = frame(&mut app, &ctx, vec![]);
        for label in [
            "To do · 1",
            "In progress · 0",
            "In review · 1",
            "Needs attention · 0",
            "Done · 1",
        ] {
            assert!(text_position(&output, label).is_some(), "missing {label}");
        }
        assert!(
            text_position(&output, "Unique story detail 1").is_none(),
            "Details must not appear beneath the board"
        );
        for shape in &output.shapes {
            if let egui::Shape::Rect(rect) = &shape.shape
                && rect.corner_radius.nw == 8
                && rect.fill == crate::ui::theme::BG
            {
                assert!(
                    rect.rect.right() <= 1773.0,
                    "Board column overflows the main panel: {:?}",
                    rect.rect
                );
            }
        }
        let click = text_position(&output, "Review task").unwrap();
        frame(
            &mut app,
            &ctx,
            vec![
                egui::Event::PointerMoved(click),
                egui::Event::PointerButton {
                    pos: click,
                    button: egui::PointerButton::Primary,
                    pressed: true,
                    modifiers: Default::default(),
                },
            ],
        );
        frame(
            &mut app,
            &ctx,
            vec![egui::Event::PointerButton {
                pos: click,
                button: egui::PointerButton::Primary,
                pressed: false,
                modifiers: Default::default(),
            }],
        );
        let output = frame(&mut app, &ctx, vec![]);
        assert_eq!(
            ctx.data_mut(|d| d.get_temp::<String>(egui::Id::new("packet_selected_task")))
                .as_deref(),
            Some(".kool-ade-packet/planning/tasks/fixture/002-task.md")
        );
        assert!(
            text_position(&output, "Unique story detail 1").is_none(),
            "texts: {:?}",
            output
                .shapes
                .iter()
                .filter_map(|shape| if let egui::Shape::Text(t) = &shape.shape {
                    Some((t.galley.text(), t.pos))
                } else {
                    None
                })
                .collect::<Vec<_>>()
        );
        let link = text_position(&output, "Open PR").unwrap();
        frame(
            &mut app,
            &ctx,
            vec![
                egui::Event::PointerMoved(link),
                egui::Event::PointerButton {
                    pos: link,
                    button: egui::PointerButton::Primary,
                    pressed: true,
                    modifiers: Default::default(),
                },
            ],
        );
        let output = frame(
            &mut app,
            &ctx,
            vec![egui::Event::PointerButton {
                pos: link,
                button: egui::PointerButton::Primary,
                pressed: false,
                modifiers: Default::default(),
            }],
        );
        assert!(output.platform_output.commands.iter().any(|cmd| matches!(cmd, egui::OutputCommand::OpenUrl(url) if url.url == "https://github.com/fixture/repo/pull/1")));
        frame(
            &mut app,
            &ctx,
            vec![egui::Event::Key {
                key: egui::Key::Escape,
                physical_key: None,
                pressed: true,
                repeat: false,
                modifiers: Default::default(),
            }],
        );
        let output = frame(&mut app, &ctx, vec![]);
        assert!(text_position(&output, "Unique story detail 1").is_none());
        assert!(
            ctx.data_mut(|d| d.get_temp::<String>(egui::Id::new("packet_selected_task")))
                .is_none()
        );
        app.implement_task(".kool-ade-packet/planning/tasks/fixture/002-task.md".into());
        assert!(
            !app.is_busy(),
            "published tasks must not start another agent"
        );
    }
    #[test]
    fn live_card_opens_full_activity_and_returns_to_item_details() {
        let mut app = fixture();
        let ticket = ".kool-ade-packet/planning/tasks/fixture/001-task.md".to_owned();
        if let Screen::Connected(p) = &mut app.screen {
            p.active_implementations.insert(
                ticket.clone(),
                crate::core::implementation::Controller::idle_fixture(),
            );
            p.activity.tasks.insert(
                ticket.clone(),
                crate::harness::LiveProgress {
                    thoughts: "Checking the permissions test results".into(),
                    activity: Some("Running tests".into()),
                    ..Default::default()
                },
            );
        }
        let ctx = egui::Context::default();
        frame(&mut app, &ctx, vec![]);
        let output = frame(&mut app, &ctx, vec![]);
        assert!(text_position(&output, "LIVE ACTIVITY").is_none());
        assert!(text_position(&output, "Checking the permissions test results").is_none());
        let click = |app: &mut PacketApp, pos: egui::Pos2| {
            for pressed in [true, false] {
                frame(
                    app,
                    &ctx,
                    vec![
                        egui::Event::PointerMoved(pos),
                        egui::Event::PointerButton {
                            pos,
                            button: egui::PointerButton::Primary,
                            pressed,
                            modifiers: Default::default(),
                        },
                    ],
                );
            }
        };
        click(&mut app, text_position(&output, "First task").unwrap());
        frame(&mut app, &ctx, vec![]);
        let output = frame(&mut app, &ctx, vec![]);
        assert!(text_position(&output, "Activity").is_some());
        assert!(text_position(&output, "Checking the permissions test results").is_some());
        click(
            &mut app,
            text_position(&output, "View all activity").unwrap(),
        );
        frame(&mut app, &ctx, vec![]);
        let output = frame(&mut app, &ctx, vec![]);
        assert!(text_position(&output, "TASK-001 / All activity").is_some());
        assert!(
            app.live_progress().is_none(),
            "Task activity must not leak to main chat"
        );
        frame(
            &mut app,
            &ctx,
            vec![egui::Event::Key {
                key: egui::Key::Escape,
                physical_key: None,
                pressed: true,
                repeat: false,
                modifiers: Default::default(),
            }],
        );
        let output = frame(&mut app, &ctx, vec![]);
        assert!(
            ctx.data_mut(|d| d.get_temp::<String>(egui::Id::new("packet_task_activity")))
                .is_none()
        );
        assert!(text_position(&output, "TASK-001 / Task details").is_some());
    }

    #[test]
    fn planning_items_use_board_and_modal_even_before_tasks_exist() {
        let mut app = fixture();
        let item = OpenItem::new(
            "CLR-010".into(),
            crate::domain::item::Priority::High,
            crate::domain::item::ItemKind::Question,
            "General".into(),
            Some("All".into()),
            "Which users need access?".into(),
            "Determines the access model".into(),
        );
        if let Screen::Connected(project) = &mut app.screen {
            project.task_documents.clear();
            project.state.items = vec![item.clone()];
        }
        let ctx = egui::Context::default();
        frame(&mut app, &ctx, vec![]);
        let output = frame(&mut app, &ctx, vec![]);
        assert!(text_position(&output, "To do · 1").is_some());
        assert!(text_position(&output, "Your answer needed").is_some());
        assert!(text_position(&output, "Determines the access model").is_none());
        let pos = text_position(&output, &item.question).unwrap();
        for pressed in [true, false] {
            frame(
                &mut app,
                &ctx,
                vec![
                    egui::Event::PointerMoved(pos),
                    egui::Event::PointerButton {
                        pos,
                        button: egui::PointerButton::Primary,
                        pressed,
                        modifiers: Default::default(),
                    },
                ],
            );
        }
        let output = frame(&mut app, &ctx, vec![]);
        assert!(text_position(&output, "Determines the access model").is_some());
        assert!(text_position(&output, "Owner: All").is_none());
        assert!(text_position(&output, "Your answer needed").is_some());
        *app.task_draft("CLR-010").unwrap() = "Use corporate SSO".into();
        assert!(app.chat_draft().is_empty());
        assert!(
            ctx.data_mut(|d| d.get_temp::<String>(egui::Id::new("packet_selected_planning")))
                .is_some()
        );
        if let Screen::Connected(project) = &mut app.screen {
            project.state.items.clear();
        }
        let output = frame(&mut app, &ctx, vec![]);
        assert!(text_position(&output, &item.question).is_none());
    }

    #[test]
    fn human_decision_brief_shows_issue_specific_buttons_and_advisory_details() {
        let mut app = fixture();
        let mut item = OpenItem::new(
            "CLR-012".into(),
            crate::domain::Priority::High,
            crate::domain::ItemKind::Question,
            "Security".into(),
            Some("Security Owner".into()),
            "Should saved sessions expire automatically?".into(),
            "The session policy affects both security and client refresh behavior.".into(),
        );
        item.decision_brief = Some(crate::domain::DecisionBrief {
            id: item.id.clone(),
            question: item.question.clone(),
            why_now: "The release flow depends on this session policy.".into(),
            recommendation: Some(crate::domain::DecisionRecommendation {
                option_id: "short-session".into(),
                rationale: "The repository records a risk from long-lived sessions.".into(),
            }),
            confidence: Some(crate::domain::DecisionConfidence {
                level: crate::domain::ConfidenceLevel::Medium,
                explanation: "Current behavior is known; user tolerance is not.".into(),
            }),
            options: vec![
                crate::domain::DecisionOption {
                    id: "short-session".into(),
                    label: "Expire sessions after one hour".into(),
                    summary: "Require users to sign in again after one hour.".into(),
                    benefits: vec!["Limits how long a stolen session remains useful.".into()],
                    costs: vec!["Users may need to sign in during longer work.".into()],
                    risks: vec!["A failed sign-in can interrupt active work.".into()],
                    consequences: vec!["All clients need to handle session expiry.".into()],
                    reversibility: "The timeout can be changed later.".into(),
                },
                crate::domain::DecisionOption {
                    id: "persistent".into(),
                    label: "Keep sessions until sign-out".into(),
                    summary: "Sessions remain active until users sign out.".into(),
                    benefits: vec!["Users avoid repeat sign-ins.".into()],
                    costs: vec!["Revocation remains the normal way to end access.".into()],
                    risks: vec!["A stolen session stays useful longer.".into()],
                    consequences: vec!["The current client behavior stays familiar.".into()],
                    reversibility: "A future expiry policy would require client changes.".into(),
                },
            ],
            benefits: vec![],
            costs: vec![],
            risks: vec![],
            ramifications: vec!["Every signed-in client follows the selected policy.".into()],
            reversibility: "The policy can be revisited after clients support it.".into(),
            defer_consequence: "The authentication contract remains unfinished.".into(),
            evidence: vec!["src/auth/session.rs records the current behavior.".into()],
            adr_assessment: Some(crate::domain::AdrAssessment {
                create: false,
                title: String::new(),
                rationale: "This choice is not durable enough to need an ADR.".into(),
                revisit_when: vec![],
            }),
        });
        if let Screen::Connected(project) = &mut app.screen {
            project.task_documents.clear();
            project.state.items = vec![item.clone()];
        }
        app.cached_user = crate::domain::CurrentUser::new("Security Owner", Vec::new());
        let ctx = egui::Context::default();
        frame(&mut app, &ctx, vec![]);
        let output = frame(&mut app, &ctx, vec![]);
        for label in [
            "Expire sessions after one hour",
            "Keep sessions until sign-out",
            "Your answer needed",
            "Require users to sign in again after one hour.",
            "If chosen: All clients need to handle session expiry.",
        ] {
            assert!(
                text_position(&output, label).is_some(),
                "missing {label}; visible text: {:?}",
                output
                    .shapes
                    .iter()
                    .filter_map(|clipped| match &clipped.shape {
                        egui::Shape::Text(shape) => Some(shape.galley.text().to_owned()),
                        _ => None,
                    })
                    .collect::<Vec<_>>()
            );
        }
        click_text(&mut app, &ctx, "Expire sessions after one hour");
        assert_eq!(
            app.task_draft(&item.id).map(|draft| draft.as_str()),
            Some("I choose option (short-session): Expire sessions after one hour.")
        );
        if let Screen::Connected(project) = &app.screen {
            assert_eq!(
                project.state.items[0].status,
                crate::domain::ItemStatus::Open
            );
            assert!(project.active_turn.is_none());
        }

        let details = click_text(&mut app, &ctx, &item.question);
        assert!(text_position(&details, "Decision guidance").is_some());
        assert!(text_contains(
            &details,
            "Packet recommends Expire sessions after one hour"
        ));
        assert!(text_position(&details, "Decision details").is_some());
        let _ = click_text(&mut app, &ctx, "Decision details");
        let details = click_text(
            &mut app,
            &ctx,
            "Expire sessions after one hour · Require users to sign in again after one hour.",
        );
        for label in [
            "Confidence: Medium",
            "Limits how long a stolen session remains useful.",
            "A failed sign-in can interrupt active work.",
            "Every signed-in client follows the selected policy.",
            "The authentication contract remains unfinished.",
            "src/auth/session.rs records the current behavior.",
        ] {
            assert!(
                text_contains(&details, label),
                "missing {label}; visible text: {:?}",
                details
                    .shapes
                    .iter()
                    .filter_map(|clipped| match &clipped.shape {
                        egui::Shape::Text(shape) => Some(shape.galley.text().to_owned()),
                        _ => None,
                    })
                    .collect::<Vec<_>>()
            );
        }
        assert!(text_position(&details, "Approve provisional decision").is_none());
    }

    #[test]
    fn migrated_open_item_remains_clickable_on_kanban() {
        let root = std::env::temp_dir().join(format!(
            "packet_migrated_board_{}-{}",
            std::process::id(),
            chrono::Utc::now().timestamp_nanos_opt().unwrap()
        ));
        std::fs::create_dir_all(root.join("planning")).unwrap();
        let legacy = crate::artifacts::spec_doc::bootstrap_template("Migrated product");
        std::fs::write(root.join("planning/specification.md"), &legacy).unwrap();
        let item = OpenItem::new(
            "CLR-041".into(),
            crate::domain::Priority::High,
            crate::domain::ItemKind::Question,
            "General".into(),
            Some("All".into()),
            "Who reviews saved searches?".into(),
            "Review ownership remains open.".into(),
        );
        std::fs::write(
            root.join("planning/open-items.md"),
            crate::artifacts::items_io::serialize(std::slice::from_ref(&item)),
        )
        .unwrap();
        crate::artifacts::product_docs::migrate(&root, &legacy).unwrap();
        let mut app = fixture();
        if let Screen::Connected(project) = &mut app.screen {
            project.state = crate::core::state::PlannerState::load(&root).unwrap();
            project.task_documents.clear();
        }
        let ctx = egui::Context::default();
        frame(&mut app, &ctx, vec![]);
        let output = frame(&mut app, &ctx, vec![]);
        assert!(text_position(&output, &item.question).is_some());
        let details = click_text(&mut app, &ctx, &item.question);
        assert!(text_position(&details, &item.reason).is_some());
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn kanban_distinguishes_authority_blockers_tasks_and_completed_work() {
        let mut app = fixture();
        let make = |id: &str, authority, priority, question: &str| {
            let mut item = OpenItem::new(
                id.into(),
                priority,
                crate::domain::ItemKind::Question,
                "General".into(),
                Some("All".into()),
                question.into(),
                "Evidence".into(),
            );
            item.authority = authority;
            item
        };
        if let Screen::Connected(project) = &mut app.screen {
            project.state.items = vec![
                make(
                    "CLR-101",
                    crate::domain::Authority::Human,
                    crate::domain::Priority::Normal,
                    "Human decision card",
                ),
                make(
                    "CLR-102",
                    crate::domain::Authority::Agent,
                    crate::domain::Priority::High,
                    "Agent resolving card",
                ),
                make(
                    "CLR-103",
                    crate::domain::Authority::Review,
                    crate::domain::Priority::Normal,
                    "Review decision card",
                ),
                make(
                    "CLR-104",
                    crate::domain::Authority::Human,
                    crate::domain::Priority::Blocking,
                    "Blocking human card",
                ),
            ];
        }
        let ctx = egui::Context::default();
        frame(&mut app, &ctx, vec![]);
        let output = frame(&mut app, &ctx, vec![]);
        for label in [
            "To do · 3",
            "In progress · 0",
            "In review · 2",
            "Needs attention · 1",
            "Done · 1",
            "Human decision card",
            "Agent resolving card",
            "Review decision card",
            "Blocking human card",
            "Human",
            "Agent",
            "Review",
            "Blocking",
        ] {
            assert!(text_position(&output, label).is_some(), "missing {label}");
        }
        assert!(text_position(&output, "Non-actionable repository observation").is_none());
    }

    #[test]
    fn agent_item_shows_live_investigation_on_board_and_in_detail() {
        let mut app = fixture();
        let mut item = OpenItem::new(
            "CLR-011".into(),
            crate::domain::Priority::Blocking,
            crate::domain::ItemKind::Ambiguity,
            "General".into(),
            Some("All".into()),
            "Does the repository already persist queries?".into(),
            "Investigate the source.".into(),
        );
        item.authority = crate::domain::Authority::Agent;
        if let Screen::Connected(project) = &mut app.screen {
            project.task_documents.clear();
            project.state.items = vec![item.clone()];
            project.activity.tasks.insert(
                item.id.clone(),
                crate::harness::LiveProgress {
                    activity: Some("Reading search source".into()),
                    response: "Found the query cache but no persistence adapter".into(),
                    thoughts: "Checking restart behavior".into(),
                    ..Default::default()
                },
            );
        }
        let ctx = egui::Context::default();
        frame(&mut app, &ctx, vec![]);
        let output = frame(&mut app, &ctx, vec![]);
        assert!(text_position(&output, "To do · 1").is_some());
        assert!(text_position(&output, "Reading search source").is_none());
        let details = click_text(&mut app, &ctx, &item.question);
        assert!(text_position(&details, "Agent investigation").is_none());
        click_text(&mut app, &ctx, "Activity");
        let details = frame(&mut app, &ctx, vec![]);
        assert!(text_position(&details, "Agent investigation").is_some());
        assert!(text_position(&details, "Worker notes").is_some());
        assert!(
            app.live_progress().is_none(),
            "Item worker output must stay out of main chat"
        );
    }

    #[test]
    fn unsupported_platform_rejects_implementation_before_dispatch() {
        let mut app = fixture();
        let capabilities = crate::harness::runtime_capabilities::RuntimeCapabilities {
            planning_access:
                crate::harness::runtime_capabilities::PlanningAccess::SuppliedContextOnly,
            implementation: false,
        };
        app.start_implementation_with_capabilities(
            ".kool-ade-packet/planning/tasks/fixture/001-task.md".into(),
            false,
            capabilities,
        );
        let Screen::Connected(project) = &app.screen else {
            panic!("disconnected");
        };
        assert!(project.active_implementations.is_empty());
        assert!(!project.queue.running);
        assert!(project.queue.in_flight.is_empty());
        assert!(
            project
                .queue
                .last_error
                .contains("Planning remains available")
        );
    }

    #[test]
    fn auto_queue_cannot_start_task_from_unapproved_feature() {
        let mut app = fixture();
        let ticket = ".kool-ade-packet/planning/tasks/fixture/001-task.md";
        if let Screen::Connected(project) = &mut app.screen {
            assert!(project.queue.auto_build);
            project.task_documents[0]
                .text
                .push_str("\nFeature ID: CHG-001\n");
        }
        app.implement_task(ticket.into());
        let Screen::Connected(project) = &app.screen else {
            panic!("disconnected");
        };
        assert!(project.active_implementations.is_empty());
        assert!(!project.queue.running);
        assert!(project.queue.last_error.contains("needs explicit approval"));
    }

    #[test]
    fn resume_dispatch_accepts_feature_named_workspace_story() {
        let _shield = crate::core::gitops::test_support::shield("resume-feature-ticket");
        let root = std::env::temp_dir().join(format!(
            "packet-resume-dispatch-{}-{}",
            std::process::id(),
            chrono::Utc::now().timestamp_nanos_opt().unwrap()
        ));
        let ticket = ".kool-ade-packet/planning/tasks/demo/CHG-003-TASK-verify.md";
        std::fs::create_dir_all(root.join(ticket).parent().unwrap()).unwrap();
        std::fs::write(root.join(ticket), "# Verify workspace\n").unwrap();
        assert!(
            std::process::Command::new("git")
                .args(["init", "-q", "-b", "main"])
                .current_dir(&root)
                .status()
                .unwrap()
                .success()
        );
        let mut app = fixture();
        if let Screen::Connected(p) = &mut app.screen {
            let mut record = p.implementation_states.values().next().unwrap().clone();
            record.ticket = ticket.into();
            record.ticket_text = "# Verify workspace\n".into();
            record.status = ImplementationStatus::Blocked;
            record.pr_url = None;
            p.state = crate::core::state::PlannerState::load(&root).unwrap();
            p.task_documents = vec![crate::artifacts::task_docs::TaskDocument {
                path: ticket.into(),
                title: "Verify workspace".into(),
                text: record.ticket_text.clone(),
                identity: None,
                metadata: None,
                metadata_error: None,
            }];
            p.implementation_states = [(ticket.into(), record)].into();
            p.queue.blocked.insert(
                ticket.into(),
                crate::core::implementation::Failure::other("Previous failure"),
            );
            p.queue.recovery_attempts.insert(ticket.into(), 1);
        }
        app.implement_task(ticket.into());
        let Screen::Connected(p) = &mut app.screen else {
            panic!("disconnected")
        };
        assert!(
            p.active_implementations.contains_key(ticket),
            "{}",
            p.queue.last_error
        );
        assert!(!p.queue.blocked.contains_key(ticket));
        assert!(!p.queue.recovery_attempts.contains_key(ticket));
        let controller = p.active_implementations.remove(ticket).unwrap();
        controller.request_cancel();
        let deadline = Instant::now() + Duration::from_secs(5);
        loop {
            if matches!(
                controller.poll(),
                Some(crate::core::implementation::Event::Done(_))
            ) {
                break;
            }
            assert!(Instant::now() < deadline, "fixture worker failed to stop");
            std::thread::sleep(Duration::from_millis(10));
        }
        // No remote is configured: this dispatch test cannot launch Pi or publish.
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn chat_action_moves_from_generation_to_implementation_and_disappears_when_done() {
        let mut app = fixture();
        if let Screen::Connected(p) = &mut app.screen {
            p.state.workflow.brief = Some(crate::core::workflow::InterviewBrief {
                feature_name: "Current feature".into(),
                ready_for_tasks: true,
                ..Default::default()
            });
            p.state.workflow.reviewed_specification =
                p.state.planning_contract().map(str::to_owned);
            p.state.workflow.task_batches.clear();
            p.state.active_feature =
                Some(("CHG-001".into(), "Current feature specification".into()));
            p.state.workflow.reviewed_specification =
                p.state.planning_contract().map(str::to_owned);
        }
        assert!(app.task_offer().is_some());
        assert!(!app.implementation_offer());
        if let Screen::Connected(p) = &mut app.screen {
            p.state
                .workflow
                .task_batches
                .push(crate::core::workflow::TaskBatchRef {
                    identity: None,
                    feature: "Current feature".into(),
                    directory: ".kool-ade-packet/planning/tasks/fixture".into(),
                    count: 3,
                });
        }
        assert!(
            app.task_offer().is_none(),
            "stale readiness must not offer duplicate generation"
        );
        assert!(app.implementation_offer());
        let ctx = egui::Context::default();
        frame(&mut app, &ctx, vec![]);
        let output = frame(&mut app, &ctx, vec![]);
        assert!(text_position(&output, "Implement tasks").is_some());
        assert!(text_position(&output, "Generate task stories").is_none());
        if let Screen::Connected(p) = &mut app.screen {
            let mut done = p.implementation_states.values().next().unwrap().clone();
            done.status = ImplementationStatus::Completed;
            for doc in &p.task_documents {
                p.implementation_states
                    .insert(doc.path.clone(), done.clone());
            }
        }
        assert!(!app.implementation_offer());
        assert!(app.task_offer().is_none());
    }

    #[test]
    fn typed_start_action_without_tasks_explains_the_live_blocker() {
        let mut app = fixture();
        if let Screen::Connected(p) = &mut app.screen {
            p.task_documents.clear();
        }
        requested_action::dispatch(
            &mut app,
            crate::harness::RequestedAction {
                action: crate::harness::ApplicationAction::StartImplementation,
                target_uid: None,
            },
        );
        let Screen::Connected(p) = &app.screen else {
            panic!("disconnected")
        };
        assert!(p.active_turn.is_none());
        assert!(p.active_implementations.is_empty());
        assert!(!p.queue.running);
        assert!(
            app.chat_messages()
                .last()
                .unwrap()
                .text
                .contains("There is no task batch")
        );
    }

    #[test]
    fn parallel_cards_and_targeted_cancel_preserve_other_workers() {
        let mut app = fixture();
        let first = ".kool-ade-packet/planning/tasks/fixture/001-task.md";
        let second = ".kool-ade-packet/planning/tasks/fixture/002-task.md";
        if let Screen::Connected(p) = &mut app.screen {
            p.implementation_states.remove(second);
            p.queue.max_parallel = 2;
            p.queue.running = true;
            for ticket in [first, second] {
                p.active_implementations.insert(
                    ticket.into(),
                    crate::core::implementation::Controller::idle_fixture(),
                );
            }
        }
        assert!(!app.implementation_capacity());
        assert!(app.implementation_active(first) && app.implementation_active(second));
        let ctx = egui::Context::default();
        frame(&mut app, &ctx, vec![]);
        let output = frame(&mut app, &ctx, vec![]);
        assert!(text_position(&output, "In progress · 2").is_some());
        app.cancel_task_for(first);
        if let Screen::Connected(p) = &app.screen {
            assert!(!p.queue.running);
            assert!(p.active_implementations[first].cancellation_requested());
            assert!(!p.active_implementations[second].cancellation_requested());
        }
    }

    fn park_fixture_with_unapproved_feature(lapsed: bool) -> PacketApp {
        let mut app = fixture();
        // Queue locking and persistence run `git rev-parse` against the repo
        // root, so promote the throwaway fixture directory to a real (bare
        // minimum) repository before driving the auto queue.
        let root = std::env::temp_dir().join("packet-board-ui-fixture-nonexistent");
        std::fs::create_dir_all(&root).expect("fixture root");
        assert!(
            std::process::Command::new("git")
                .args(["init", "-q"])
                .current_dir(&root)
                .status()
                .unwrap()
                .success(),
            "fixture root must initialize as a git repository"
        );
        if let Screen::Connected(p) = &mut app.screen {
            p.queue.running = true;
            if lapsed {
                p.state
                    .workflow
                    .approved_features
                    .insert("CHG-999".into(), "fixture stale contract".into());
            }
            for doc in &mut p.task_documents {
                doc.text.push_str("\nFeature ID: CHG-999\n");
            }
        }
        app
    }

    #[test]
    fn parked_auto_queue_names_the_missing_feature_approval() {
        let _shield = crate::core::gitops::test_support::shield("auto-queue-park");
        let mut app = park_fixture_with_unapproved_feature(false);
        app.advance_auto_queue();
        let Screen::Connected(p) = &app.screen else {
            panic!("screen disconnected")
        };
        assert!(!p.queue.running, "queue must park, not spin");
        assert!(
            p.active_implementations.is_empty(),
            "no worker may start for an unapproved feature"
        );
        assert!(
            p.queue.last_error.contains("CHG-999"),
            "park message must name the blocking feature id: {}",
            p.queue.last_error
        );
        assert!(
            p.queue.last_error.contains("no recorded approval")
                && p.queue
                    .last_error
                    .contains("Approve feature for implementation"),
            "park message must state the remedy: {}",
            p.queue.last_error
        );
        assert!(
            p.queue
                .last_error
                .contains(".kool-ade-packet/planning/tasks/fixture/001-task.md"),
            "park message must name the affected tickets: {}",
            p.queue.last_error
        );
        assert!(
            !p.queue
                .last_error
                .contains(".kool-ade-packet/planning/tasks/fixture/003-task.md"),
            "completed tasks must not produce stale approval blockers: {}",
            p.queue.last_error
        );
    }

    #[test]
    fn parked_auto_queue_flags_lapsed_approvals_as_reapproval_targets() {
        let _shield = crate::core::gitops::test_support::shield("auto-queue-park-lapsed");
        let mut app = park_fixture_with_unapproved_feature(true);
        app.advance_auto_queue();
        let Screen::Connected(p) = &app.screen else {
            panic!("screen disconnected")
        };
        assert!(!p.queue.running);
        assert!(
            p.queue
                .last_error
                .contains("approval lapsed after the feature document changed")
                && p.queue
                    .last_error
                    .contains("re-run Approve feature for implementation"),
            "lapsed approval must read as a re-approval target, not a wall: {}",
            p.queue.last_error
        );
    }

    #[test]
    fn auto_queue_runs_independent_tasks_concurrently_then_merges_before_dependents() {
        let _shield = crate::core::gitops::test_support::shield("auto-queue-e2e");
        struct Restore(Option<std::ffi::OsString>);
        impl Drop for Restore {
            fn drop(&mut self) {
                unsafe {
                    match self.0.take() {
                        Some(value) => std::env::set_var("PACKET_PI_BIN", value),
                        None => std::env::remove_var("PACKET_PI_BIN"),
                    }
                }
            }
        }
        let _restore = Restore(std::env::var_os("PACKET_PI_BIN"));
        struct RestorePath(Option<std::ffi::OsString>);
        impl Drop for RestorePath {
            fn drop(&mut self) {
                unsafe {
                    match self.0.take() {
                        Some(value) => std::env::set_var("PATH", value),
                        None => std::env::remove_var("PATH"),
                    }
                }
            }
        }
        let _restore_path = RestorePath(std::env::var_os("PATH"));
        let root = std::env::temp_dir().join(format!(
            "packet-auto-e2e-{}",
            chrono::Utc::now().timestamp_nanos_opt().unwrap()
        ));
        let repo = root.join("repo");
        let remote = root.join("remote.git");
        std::fs::create_dir_all(repo.join(".kool-ade-packet/planning/tasks/fixture")).unwrap();
        std::fs::create_dir_all(repo.join(".kool-ade-packet/state")).unwrap();
        let git = |cwd: &std::path::Path, args: &[&str]| {
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
            String::from_utf8(output.stdout).unwrap().trim().to_string()
        };
        git(&repo, &["init", "-q", "-b", "main"]);
        git(&repo, &["config", "user.name", "Fixture"]);
        git(&repo, &["config", "user.email", "fixture@example.test"]);
        let docs = (1..=3).map(|number| crate::artifacts::task_docs::TaskDocument {
            path: format!(".kool-ade-packet/planning/tasks/fixture/{number:03}-task.md"), title: format!("Task {number}"),
            text: format!("# Task {number}\n\n## Dependencies\n{}\n\n## Acceptance criteria\n- File exists.\n", if number == 3 { "- [Task 001](001-task.md) must be complete.\n- [Task 002](002-task.md) must be complete." } else { "None." }),
            identity: None,
                metadata: None,
                metadata_error: None,
        }).collect::<Vec<_>>();
        for doc in &docs {
            std::fs::create_dir_all(repo.join(&doc.path).parent().unwrap()).unwrap();
            std::fs::write(repo.join(&doc.path), &doc.text).unwrap();
        }
        let feature = "# CHG-001: Fixture\n\n**Status:** Ready\n\n## Intent\nFixture.\n\n## Current Behavior\nFixture.\n\n## Desired Behavior\nFixture.\n\n## Scope\nFixture.\n\n## Affected Product Areas\nFixture.\n\n## Requirements\nFixture.\n\n## Decisions and Assumptions\nFixture.\n\n## Acceptance Criteria\nFixture.\n";
        std::fs::create_dir_all(repo.join(".kool-ade-packet/planning/changes/CHG-001-fixture"))
            .unwrap();
        std::fs::write(
            repo.join(".kool-ade-packet/planning/changes/CHG-001-fixture/specification.md"),
            feature,
        )
        .unwrap();
        std::fs::write(repo.join(".kool-ade-packet/state/workflow.json"), serde_json::json!({"brief":null,"reviewedSpecification":null,"taskBatches":[{"feature":"fixture","directory":".kool-ade-packet/planning/tasks/fixture","count":3}]}).to_string()).unwrap();
        git(&repo, &["add", "."]);
        git(&repo, &["commit", "-qm", "baseline"]);
        git(&root, &["init", "--bare", "-q", remote.to_str().unwrap()]);
        let github_remote = "https://github.com/packet-fixture/fixture.git";
        git(&repo, &["remote", "add", "origin", github_remote]);
        git(
            &repo,
            &[
                "config",
                &format!("url.{}.insteadOf", remote.display()),
                github_remote,
            ],
        );
        git(&repo, &["push", "-q", "origin", "main"]);
        let fake_bin = root.join("fake-bin");
        std::fs::create_dir_all(&fake_bin).unwrap();
        let fake_gh = fake_bin.join("gh");
        std::fs::write(
            &fake_gh,
            "#!/bin/sh\ncommit=''\nwhile [ \"$#\" -gt 0 ]; do\n  if [ \"$1\" = --commit ]; then shift; commit=$1; fi\n  shift\ndone\nprintf '[{\"headSha\":\"%s\",\"status\":\"completed\",\"conclusion\":\"success\",\"workflowName\":\"fixture\",\"createdAt\":\"2099-01-01T00:00:00Z\",\"url\":\"https://github.com/packet-fixture/fixture/actions/runs/1\"}]\\n' \"$commit\"\n",
        )
        .unwrap();
        std::fs::set_permissions(&fake_gh, std::fs::Permissions::from_mode(0o700)).unwrap();
        let test_path = std::env::join_paths(std::iter::once(fake_bin.clone()).chain(
            std::env::split_paths(&std::env::var_os("PATH").unwrap_or_default()),
        ))
        .unwrap();
        unsafe { std::env::set_var("PATH", test_path) };
        // Planning approval commonly exists only in the planning-root checkout
        // when Auto starts. Its commit must remain an ancestor of published work.
        std::fs::write(
            repo.join(".kool-ade-packet/planning/local-approval.md"),
            "approved locally\n",
        )
        .unwrap();
        git(
            &repo,
            &["add", ".kool-ade-packet/planning/local-approval.md"],
        );
        git(&repo, &["commit", "-qm", "approve local plan"]);
        let pi = root.join("pi-fixture");
        std::fs::write(&pi, r#"#!/usr/bin/python3
import json, pathlib, sys, time
if '--help' in sys.argv:
    print('''--print
--mode <mode> text, json, rpc
--no-session
--no-approve
--append-system-prompt
--thinking <level> xhigh
--no-extensions
--no-skills
--no-prompt-templates
--no-context-files
--no-tools
--tools
--no-builtin-tools
--extension''')
    sys.exit(0)
prompt = sys.stdin.read()
if prompt.startswith('PROJECT MANAGER UPDATE'):
    assert '--no-tools' in sys.argv
    print(json.dumps({'type':'agent_end','messages':[{'role':'assistant','content':[{'type':'text','text':'Manager fixture: I am monitoring the assigned worker.'}]}]}))
    sys.exit(0)
if 'TICKET PATH: ' not in prompt:
    report = {'schemaVersion':1,'assistantMessage':'Planning fixture: I can discuss this while the worker runs.','openItemsAdded':[],'openItemsUpdated':[],'openItemsResolved':[]}
    print(json.dumps({'type':'agent_end','messages':[{'role':'assistant','content':[{'type':'text','text':json.dumps(report)}]}]}))
    sys.exit(0)
time.sleep(0.3)
ticket = prompt.split('TICKET PATH: ', 1)[1].splitlines()[0]
name = pathlib.Path(ticket).stem + '.txt'
root = pathlib.Path(__file__).parent
if name in ('001-task.txt', '002-task.txt'):
    (root / (name + '.started')).touch()
    deadline = time.monotonic() + 10
    while not all((root / (n + '.started')).exists() for n in ('001-task.txt', '002-task.txt')):
        assert time.monotonic() < deadline, 'Independent workers did not overlap'
        time.sleep(0.02)
else:
    assert pathlib.Path('001-task.txt').exists() and pathlib.Path('002-task.txt').exists(), 'Dependencies were not merged before starting'
pathlib.Path(name).write_text('implemented')
report = {'status':'complete','summary':'Implemented fixture task','acceptance_criteria':[{'criterion':'File exists.','evidence':'File exists and was verified'}],'verification':['test -f '+name],'remaining':[]}
print(json.dumps({'type':'agent_end','messages':[{'role':'assistant','stopReason':'stop','content':[{'type':'text','text':json.dumps(report)}]}]}))
"#).unwrap();
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&pi, std::fs::Permissions::from_mode(0o700)).unwrap();
        unsafe {
            std::env::set_var("PACKET_PI_BIN", &pi);
        }
        let mut app = fixture();
        if let Screen::Connected(project) = &mut app.screen {
            project.state = crate::core::state::PlannerState::load(&repo).unwrap();
            project.task_documents = docs.clone();
            project.queue.max_parallel = 2;
            project.queue.auto_publish = true;
            project.queue.blocked.insert(
                docs[1].path.clone(),
                crate::core::implementation::Failure::new(
                    crate::core::implementation::FailureKind::RemoteDiverged,
                    crate::core::implementation::RecoveryDisposition::AutomaticRetry,
                    "Local main and freshly fetched origin/main diverged before publication; verified work is preserved",
                ),
            );
            project.implementation_states.clear();
            project.chat_slug = format!("auto-e2e-{}", std::process::id());
        }
        // Exercise the user's actual chat action, including durable approval,
        // worker dispatch, concurrent execution and integration into the remote.
        if let Screen::Connected(project) = &mut app.screen {
            project.state.active_feature = Some(("CHG-001".into(), feature.into()));
            assert!(!crate::core::workflow::feature_approved(
                &repo,
                &project.state.workflow,
                "CHG-001"
            ));
        }
        let ctx = egui::Context::default();
        frame(&mut app, &ctx, vec![]);
        click_text(&mut app, &ctx, "Implement tasks");
        if let Screen::Connected(project) = &app.screen {
            assert!(project.active_turn.is_none());
            assert!(project.active_implementations.contains_key(&docs[0].path));
            assert!(crate::core::workflow::feature_approved(
                &repo,
                &project.state.workflow,
                "CHG-001"
            ));
            let saved = crate::core::state::PlannerState::load(&repo).unwrap();
            assert!(crate::core::workflow::feature_approved(
                &repo,
                &saved.workflow,
                "CHG-001"
            ));
        }
        let ctx = egui::Context::default();
        let deadline = Instant::now() + Duration::from_secs(40);
        let mut concurrent_chat = false;
        let mut max_workers = 0;
        loop {
            app.tick(0.1, &ctx);
            max_workers = max_workers.max(app.active_task_count());
            assert!(app.active_task_count() <= 2);
            if !concurrent_chat
                && app
                    .chat_messages()
                    .iter()
                    .any(|m| m.text.starts_with("Manager fixture:"))
            {
                assert!(
                    matches!(&app.screen, Screen::Connected(p) if !p.active_implementations.is_empty())
                );
                assert!(!app.conversation_busy());
                app.start_turn("Can we discuss planning while the task runs?");
                assert!(app.conversation_busy());
                concurrent_chat = true;
            }
            let finished = matches!(&app.screen, Screen::Connected(project) if !project.queue.running && project.active_implementations.is_empty());
            if finished {
                break;
            }
            assert!(
                Instant::now() < deadline,
                "queue did not finish: {}",
                app.queue_status()
            );
            std::thread::sleep(Duration::from_millis(20));
        }
        if let Screen::Connected(project) = &app.screen {
            assert_eq!(max_workers, 2, "Independent implementations must overlap");
            assert_eq!(project.queue.recovery_attempts.get(&docs[1].path), Some(&1));
            assert!(
                project.queue.blocked.is_empty(),
                "unexpected queue blockers: {:?}",
                project.queue.blocked
            );
            assert!(
                project.queue.last_error.is_empty(),
                "{}",
                project.queue.last_error
            );
            for doc in &docs {
                assert_eq!(
                    project.implementation_states.get(&doc.path).unwrap().status,
                    ImplementationStatus::Completed
                );
            }
        }
        assert!(
            concurrent_chat,
            "Manager should proactively engage during task execution"
        );
        assert!(
            app.chat_messages()
                .iter()
                .any(|m| m.text == "Planning fixture: I can discuss this while the worker runs.")
        );
        for doc in &docs {
            let progress = crate::core::implementation::load_activity(&repo, &doc.path).unwrap();
            assert_eq!(progress.activity.as_deref(), Some("Done"));
            assert!(!progress.response.contains("Manager fixture"));
            assert!(!progress.response.contains("Planning fixture"));
        }
        assert_eq!(git(&remote, &["rev-list", "--count", "main"]), "7");
        assert_eq!(
            git(
                &remote,
                &["show", "main:.kool-ade-packet/planning/local-approval.md"],
            ),
            "approved locally"
        );
        assert_eq!(git(&remote, &["show", "main:001-task.txt"]), "implemented");
        assert_eq!(git(&remote, &["show", "main:002-task.txt"]), "implemented");
        assert_eq!(git(&remote, &["show", "main:003-task.txt"]), "implemented");
        assert!(
            !crate::core::implementation_queue::Queue::load(&repo)
                .unwrap()
                .running
        );
        drop(app);
        std::fs::remove_dir_all(&root).unwrap();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// AC5 (literal chain): the display caches (panel highlighting, chat
    /// eligibility) report the GIT-DERIVED seat from `effective_user` — not
    /// the config block's contradicting name and not the project title. Then
    /// an EXTERNAL git edit of local `user.name` → a settings Save (which
    /// resyncs) → the refreshed display cache reports the NEW seat — proving
    /// derivation runs after settings-Save, not only at connect.
    #[test]
    fn derive_caches_projects_the_git_derived_seat_not_third_identities() {
        let _shield = crate::core::gitops::test_support::shield("caches-mira");
        let root = std::env::temp_dir().join(format!("packet_caches_mira_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(&root).unwrap();
        let git = |args: &[&str]| -> std::process::Output {
            std::process::Command::new("git")
                .arg("-C")
                .arg(&root)
                .args(args)
                .output()
                .unwrap()
        };
        git(&["init", "-q", "-b", "main"]);
        git(&["config", "user.name", "Sam Lee"]);
        git(&["config", "user.email", "sam@example.org"]);
        let config = root.join(".kool-ade-packet/config");
        std::fs::create_dir_all(&config).unwrap();
        std::fs::write(
            config.join("project.md"),
            "# Planner Configuration\n\n## Current User\nName: Bob\nGroups: Ops\n\n## Stakeholders\n\n### QA\n(no owner configured)\n",
        )
        .unwrap();

        let state = crate::core::state::PlannerState::load(&root).unwrap();
        let mut proj = Project {
            task_chats: Default::default(),
            activity: Default::default(),
            state,
            chat_slug: "test-slug".into(),
            chat: Vec::new(),
            draft: String::new(),
            queue: Default::default(),
            queue_lock: None,
            active_implementations: Default::default(),
            pr_refresh: None,
            reconciliation: Default::default(),
            investigation: None,
            investigation_attempted: Default::default(),
            investigation_cooldown_until: None,
            last_pr_refresh: None,
            implementation_states: Default::default(),
            active_turn: None,
            task_turns: Default::default(),
            task_live: Default::default(),
            planning_work: Default::default(),
            active_planning_work: None,
            live_progress: Default::default(),
            next_question_id: None,
            git: Default::default(),
            task_documents: Vec::new(),
            archived_tasks: Default::default(),
        };
        // Connect: the git seat is projected — not the block's 'Bob',
        // not the project title.
        let (cached, _eligible) = PacketApp::derive_caches(&proj);
        assert_eq!(cached.name, "Sam Lee");
        assert_eq!(cached.groups, vec!["Ops".to_string()]);
        assert_eq!(
            proj.state.identity.source,
            crate::domain::user::IdentitySource::GitUserName
        );

        // External edit of the local git identity takes effect only on the
        // next resync — the cache is still stale before the save.
        git(&["config", "user.name", "Mira Chen"]);
        let (stale, _eligible) = PacketApp::derive_caches(&proj);
        assert_eq!(stale.name, "Sam Lee");

        // AC5: a no-edit settings Save (write → resync → checkpoint) re-
        // derives the seat, and the refreshed display cache follows.
        let mut dlg = crate::app::dialogs::DlgSettings::from_project(&proj);
        assert_eq!(dlg.user_name, "Sam Lee");
        dlg.apply(&mut proj)
            .expect("no-edit settings save must succeed");
        let (resynced, _eligible) = PacketApp::derive_caches(&proj);
        assert_eq!(resynced.name, "Mira Chen");
        assert_eq!(
            proj.state.identity.source,
            crate::domain::user::IdentitySource::GitUserName
        );
        let log = String::from_utf8_lossy(&git(&["log", "-1", "--pretty=%s"]).stdout).into_owned();
        assert!(
            log.contains("settings: update workspace settings"),
            "checkpoint subject: {log}"
        );
        let _ = std::fs::remove_dir_all(&root);
    }

    // ---- CHG-003 welcome screen ↔ workspace-browser dialog -----------------
    //
    // House practice (F-16): the app pump needs an eframe::Frame (GPU
    // object), so tests drive the two production pieces directly —
    // `welcome::paint` for the button and `PacketApp::render_dialog` for
    // the dialog contract — exactly how the other dialogs are exercised in
    // this codebase.

    /// Route one frame of the PARKED dialog through the production router.
    fn sw_route(
        ctx: &egui::Context,
        app: &mut PacketApp,
        events: Vec<egui::Event>,
    ) -> egui::FullOutput {
        let mut parked = Some(
            app.dialog
                .take()
                .expect("a dialog is parked for this route"),
        );
        for theme in [egui::Theme::Dark, egui::Theme::Light] {
            ctx.style_mut_of(theme, |style| style.animation_time = 0.0);
        }
        let mut out = ctx.run_ui(
            egui::RawInput {
                screen_rect: Some(egui::Rect::from_min_size(
                    egui::Pos2::ZERO,
                    egui::vec2(1400.0, 900.0),
                )),
                events,
                ..Default::default()
            },
            |ui| app.render_dialog(ui, parked.take().expect("parked dialog for this route")),
        );
        // No GPU consumer in-process: drain textures before the output dies.
        out.textures_delta.clear();
        out
    }

    /// Locate a whole-word text shape; centre position of its mesh.
    fn sw_text_pos(output: &egui::FullOutput, needle: &str) -> Option<egui::Pos2> {
        output.shapes.iter().find_map(|shape| {
            let egui::Shape::Text(text) = &shape.shape else {
                return None;
            };
            (text.galley.text() == needle)
                .then(|| text.pos + text.galley.mesh_bounds.center().to_vec2())
        })
    }

    /// Consume a fresh context's first pass, which paints placeholder (Noop)
    /// shapes only. Call once per `egui::Context` before trusting geometry —
    /// the same warm-up discipline the overlays modal tests apply. Leaves the
    /// parked dialog parked (idle frame).
    fn sw_warm_route(ctx: &egui::Context, app: &mut PacketApp) {
        let _out = sw_route(ctx, app, Vec::new());
    }

    /// One-frame click (move -> press -> release) at an absolute position.
    fn sw_route_click_at(ctx: &egui::Context, app: &mut PacketApp, pos: egui::Pos2) {
        let btn = egui::PointerButton::Primary;
        let mods = Default::default();
        let _ = sw_route(
            ctx,
            app,
            vec![
                egui::Event::PointerMoved(pos),
                egui::Event::PointerButton {
                    pos,
                    button: btn,
                    pressed: true,
                    modifiers: mods,
                },
                egui::Event::PointerButton {
                    pos,
                    button: btn,
                    pressed: false,
                    modifiers: mods,
                },
            ],
        );
    }

    /// Locate `label` by its painted text, then click it in one frame. The
    /// locating frame assumes the context has already been warmed.
    fn sw_route_click_by_label(ctx: &egui::Context, app: &mut PacketApp, label: &str) {
        let out = sw_route(ctx, app, Vec::new());
        let pos = sw_text_pos(&out, label)
            .unwrap_or_else(|| panic!("no \u{2018}{label}\u{2019} painted in the dialog frame"));
        sw_route_click_at(ctx, app, pos);
    }

    /// Centre of the modal\u{2019}s top-right close box (28x28, 17px in from the
    /// panel edges: 16 padding + 1 stroke).
    fn sw_close_pos(panel: egui::Rect) -> egui::Pos2 {
        egui::Pos2::new(panel.max.x - 31.0, panel.min.y + 31.0)
    }

    /// The \u{201c}Choose folder\u{201d} button: the modal\u{2019}s rounded-6 rect wider than a
    /// fist (colour-independent predicate; the panel frame rounds at 12).
    fn sw_choose_rect(out: &egui::FullOutput) -> egui::Rect {
        out.shapes
            .iter()
            .find_map(|sl| match &sl.shape {
                egui::Shape::Rect(r) => ((r.corner_radius.nw as f32 - 6.0).abs() < 0.51
                    && r.rect.size().x > 60.0)
                    .then_some(r.rect),
                _ => None,
            })
            .unwrap_or_else(|| panic!("Choose-folder button rect missing from painted shapes"))
    }

    #[test]
    fn sw_welcome_browse_button_signals_request_when_clicked() {
        let mut conn = String::new();
        let mut gh = String::new();
        let mut clone_flag = false;
        let mut req = false;
        let ctx = egui::Context::default();
        let screen = egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(1400.0, 900.0));

        let mut idle = |req_out: &mut bool| {
            let mut out = ctx.run_ui(
                egui::RawInput {
                    screen_rect: Some(screen),
                    events: vec![],
                    ..Default::default()
                },
                |ui| {
                    crate::app::welcome::paint(
                        ui,
                        &mut conn,
                        &mut gh,
                        None,
                        req_out,
                        &mut clone_flag,
                        None,
                        None,
                        None,
                    );
                },
            );
            out.textures_delta.clear(); // headless: no GPU consumer
            out
        };

        // A fresh context\u{2019}s first pass is placeholders only: burn it,
        // then probe for the Browse button \u{2014} the only ~96x42 rect on the bare
        // welcome surface.
        idle(&mut req);
        assert!(!req, "an idle frame makes no request");
        let out = idle(&mut req);
        let btn = out
            .shapes
            .iter()
            .find_map(|sl| match &sl.shape {
                egui::Shape::Rect(r) => (((r.rect.size().x - 96.0).abs() < 2.01)
                    && ((r.rect.size().y - 42.0).abs() < 2.01))
                    .then_some(r.rect),
                _ => None,
            })
            .unwrap_or_else(|| panic!("Browse button rect missing from painted shapes"));
        assert!(
            btn.intersects(screen),
            "the button sits inside the viewport"
        );
        let pos = btn.center();

        // Acting frame: press+release on the button in a single frame.
        let mut req2 = false;
        let mut clone2 = false;
        let btn_evt = egui::PointerButton::Primary;
        let mods = Default::default();
        let mut acted = ctx.run_ui(
            egui::RawInput {
                screen_rect: Some(screen),
                events: vec![
                    egui::Event::PointerMoved(pos),
                    egui::Event::PointerButton {
                        pos,
                        button: btn_evt,
                        pressed: true,
                        modifiers: mods,
                    },
                    egui::Event::PointerButton {
                        pos,
                        button: btn_evt,
                        pressed: false,
                        modifiers: mods,
                    },
                ],
                ..Default::default()
            },
            |ui| {
                crate::app::welcome::paint(
                    ui,
                    &mut conn,
                    &mut gh,
                    None,
                    &mut req2,
                    &mut clone2,
                    None,
                    None,
                    None,
                );
            },
        );
        acted.textures_delta.clear(); // headless: no GPU consumer
        assert!(
            req2,
            "clicking Browse\u{2026} raises the one-shot request flag"
        );
    }

    #[test]
    fn sw_browse_choice_writes_conn_path_only_then_existing_submit_connects() {
        // Serialize ambient-environment mutations (git hierarchy + per-user
        // state root) behind the house lock while this test runs a REAL
        // connect inside a sandbox.
        let _guard = crate::core::gitops::test_support::shield("sw-browse-glue");

        let nanos = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_nanos() % 1_000_000_000_000u128)
            .unwrap_or(0);
        let root = std::env::temp_dir().join(format!("swglue-{}-{nanos}", std::process::id()));
        let pkg_home = root.join("pkghome");
        std::fs::create_dir_all(&pkg_home).unwrap();
        // SAFETY: the shield is held; no other test observes the per-user
        // state root while this one runs.
        unsafe {
            std::env::set_var("PACKET_HOME", &pkg_home);
        }

        let ws = root.join("ws");
        std::fs::create_dir_all(ws.join("plain")).unwrap();
        let repo = ws.join("site-app");
        std::fs::create_dir_all(&repo).unwrap();
        let repo_s = repo.to_string_lossy().into_owned();
        let st = std::process::Command::new("git")
            .args(["-C", &repo_s, "init", "-q"])
            .status()
            .unwrap();
        assert!(st.success(), "git init fixture failed");
        for (k, v) in [
            ("user.name", "Sw Glue Test"),
            ("user.email", "sw-glue@example.invalid"),
            ("commit.gpgsign", "false"),
        ] {
            let c = std::process::Command::new("git")
                .args(["-C", &repo_s, "config", k, v])
                .status()
                .unwrap();
            assert!(c.success(), "git config {k} failed");
        }
        // A mature worktree, the way existing operators' repos look: its
        // product specification was ALREADY migrated to modules and
        // checkpointed, so the (unchanged) connect pipeline's
        // bootstrap/migrate checkpoint deals only in files that exist.
        std::fs::create_dir_all(repo.join("planning")).unwrap();
        let template = crate::artifacts::spec_doc::bootstrap_template("Site App");
        std::fs::write(repo.join("planning/specification.md"), &template).unwrap();
        crate::artifacts::product_docs::migrate(&repo, &template)
            .expect("fixture pre-migration failed");
        for verb in [
            &["add", "-A"][..],
            &["commit", "-q", "-m", "baseline: mature workspace"][..],
        ] {
            let c = std::process::Command::new("git")
                .args(["-C", &repo_s])
                .args(verb)
                .status()
                .unwrap();
            assert!(c.success(), "fixture git step {:?} failed", verb);
        }
        let ws_s = ws.to_string_lossy().into_owned();
        let repo_c = std::fs::canonicalize(&repo).unwrap();

        let mut app = PacketApp {
            conn_path: ws_s.clone(),
            conn_error: Some(String::from("prior-error-note")),
            ..Default::default()
        };

        let ctx = egui::Context::default();

        // Park the dialog the way the welcome arm does: a fresh browser
        // seeded from the CURRENT field contents. Burn the fresh context\u{2019}s
        // placeholder-only first pass, then idle: the router re-parks and
        // nothing else moves.
        app.dialog = Some(Dialog::Browse(crate::app::dialogs::DlgBrowse::seeded(
            app.conn_path.clone(),
        )));
        sw_warm_route(&ctx, &mut app);
        let out = sw_route(&ctx, &mut app, Vec::new());
        assert!(
            app.dialog.is_some(),
            "an idle dialog frame parks the modal back"
        );
        assert_eq!(app.conn_path, ws_s, "idle frame never touches conn_path");
        assert_eq!(app.conn_error.as_deref(), Some("prior-error-note"));

        // Drive the AC: the seeded workspace lists its children (site-app, a
        // git working tree); single-click it to select, then press
        // \u{201c}Choose folder\u{201d}. Selection writes nothing until Choose.
        let site =
            sw_text_pos(&out, "site-app").expect("the browser lists the seeded workspace contents");
        let choose_at = sw_choose_rect(&out).center();
        sw_route_click_at(&ctx, &mut app, site);
        assert!(
            app.dialog.is_some(),
            "selecting alone keeps the browser open"
        );
        assert_eq!(
            app.conn_path, ws_s,
            "a single-click select is not written back"
        );
        sw_route_click_at(&ctx, &mut app, choose_at);
        assert!(
            app.dialog.is_none(),
            "a chosen dialog is consumed, not parked back"
        );
        assert_eq!(
            app.conn_path,
            repo_c.to_string_lossy(),
            "the chosen canonical path lands in the field verbatim"
        );
        assert_eq!(
            app.conn_error.as_deref(),
            Some("prior-error-note"),
            "choose never touches conn_error"
        );
        assert!(
            matches!(app.screen, Screen::Welcome),
            "choose never navigates or connects"
        );

        // From here the flow is the PRE-EXISTING submit path, unchanged:
        // Open/Enter on this field connects exactly like a hand-typed path.
        app.submit_connect();
        assert!(
            matches!(app.screen, Screen::Connected(ref p) if p.state.title == "site-app"),
            "submit_connect proceeds normally after a browse choice (err={:?})",
            app.conn_error
        );
        assert!(app.conn_error.is_none());
        // The connect persisted chat state under the ISOLATED per-user root,
        // proving the full pipeline ran inside the sandbox.
        let chatted = std::fs::read_dir(pkg_home.join("projects"))
            .map(|d| {
                d.filter_map(Result::ok).any(|e| {
                    std::fs::read_dir(e.path())
                        .map(|f| f.flatten().any(|f| f.file_name() == "chat.jsonl"))
                        .unwrap_or(false)
                })
            })
            .unwrap_or(false);
        assert!(
            chatted,
            "connected session persisted chat state under PACKET_HOME"
        );

        // Reject paths (fresh app): Cancel, the close \u{2715}, and Escape all
        // consume the dialog leaving the field byte-identical.
        let mut app2 = PacketApp {
            conn_path: ws_s.clone(),
            conn_error: Some(String::from("prior-error-note")),
            ..Default::default()
        };

        app2.dialog = Some(Dialog::Browse(crate::app::dialogs::DlgBrowse::seeded(
            ws_s.clone(),
        )));
        sw_route_click_by_label(&ctx, &mut app2, "Cancel");
        assert!(app2.dialog.is_none());
        assert_eq!(
            app2.conn_path, ws_s,
            "Cancel leaves the typed path untouched"
        );
        assert_eq!(app2.conn_error.as_deref(), Some("prior-error-note"));

        // Closing the modal \u{2715} (unlabeled X-shape): click its derived centre.
        app2.dialog = Some(Dialog::Browse(crate::app::dialogs::DlgBrowse::seeded(
            ws_s.clone(),
        )));
        let out = sw_route(&ctx, &mut app2, Vec::new());
        let panel = out
            .shapes
            .iter()
            .find_map(|sl| match &sl.shape {
                egui::Shape::Rect(r) => ((r.corner_radius.nw as f32 - 12.0).abs() < 1.01
                    && r.stroke.width >= 1.0)
                    .then_some(r.rect),
                _ => None,
            })
            .unwrap_or_else(|| panic!("modal panel frame missing from shapes"));
        sw_route_click_at(&ctx, &mut app2, sw_close_pos(panel));
        assert!(
            app2.dialog.is_none(),
            "the close \u{2715} dismisses the modal"
        );
        assert_eq!(
            app2.conn_path, ws_s,
            "close \u{2715} leaves the typed path untouched"
        );

        app2.dialog = Some(Dialog::Browse(crate::app::dialogs::DlgBrowse::seeded(
            ws_s.clone(),
        )));
        let _ = sw_route(
            &ctx,
            &mut app2,
            vec![egui::Event::Key {
                key: egui::Key::Escape,
                physical_key: None,
                modifiers: Default::default(),
                pressed: true,
                repeat: false,
            }],
        );
        assert!(app2.dialog.is_none());
        assert_eq!(
            app2.conn_path, ws_s,
            "Escape leaves the typed path untouched"
        );

        // SAFETY: restore ambient state before teardown.
        unsafe {
            std::env::remove_var("PACKET_HOME");
        }
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn sw_browse_choice_on_plain_folder_then_open_shows_legacy_invalid_repo_banner() {
        // AC4: choosing a NON-git folder travels the SAME write path as a
        // git tree (canonical PathBuf -> lossy String into conn_path) — the
        // browser neither enables nor disables it. Only the operator's
        // subsequent Open, through the UNCHANGED submit_connect ->
        // welcome::attempt_connect pipeline, reproduces the legacy
        // InvalidRepo banner.
        let nanos = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_nanos() % 1_000_000_000_000u128)
            .unwrap_or(0);
        let ws = std::env::temp_dir().join(format!("swplain-{}-{nanos}", std::process::id()));
        std::fs::create_dir_all(ws.join("plainB")).unwrap();
        let ws_s = ws.to_string_lossy().into_owned();
        let plain_c = std::fs::canonicalize(ws.join("plainB")).unwrap();

        let mut app = PacketApp {
            conn_path: ws_s.clone(),
            ..Default::default()
        };
        let ctx = egui::Context::default();
        app.dialog = Some(Dialog::Browse(crate::app::dialogs::DlgBrowse::seeded(
            ws_s.clone(),
        )));
        sw_warm_route(&ctx, &mut app);

        // Seeded listing shows plainB; single-click selects, Choose inserts.
        let out = sw_route(&ctx, &mut app, Vec::new());
        let hit = sw_text_pos(&out, "plainB").expect("plainB row painted");
        sw_route_click_at(&ctx, &mut app, hit);
        assert!(app.dialog.is_some(), "selecting keeps the browser open");
        assert_eq!(app.conn_path, ws_s, "a select writes nothing back");
        let out = sw_route(&ctx, &mut app, Vec::new());
        sw_route_click_at(&ctx, &mut app, sw_choose_rect(&out).center());
        assert!(app.dialog.is_none(), "choose consumes the dialog");
        assert_eq!(
            app.conn_path,
            plain_c.to_string_lossy(),
            "the non-git folder is inserted IDENTICALLY to a git tree"
        );
        assert!(
            matches!(app.screen, Screen::Welcome),
            "choose never navigates"
        );

        // The single connect authority runs on the operator's Open: the
        // pre-existing banner surfaces, unchanged.
        app.submit_connect();
        assert!(
            matches!(app.screen, Screen::Welcome),
            "Open refused the non-git folder: still the initial screen"
        );
        let err = app.conn_error.as_deref().unwrap_or("");
        assert!(
            err.contains("no .git directory found"),
            "legacy InvalidRepo banner reproduced (got: {err})"
        );
        assert!(
            err.contains(plain_c.to_str().unwrap_or("")),
            "banner names the chosen path (got: {err})"
        );
        let _ = std::fs::remove_dir_all(&ws);
    }

    // =================================================================
    // CHG-003: GitHub URL clone (wiring + card behaviour)
    // =================================================================

    /// Controllable stand-in for the clone worker: `compute` runs only
    /// after the gate receives. Deterministic settle-between-ticks;
    /// the join handle lets the ticker observe real settlement state.
    fn gated_worker<F>(
        compute: F,
    ) -> (
        std::thread::JoinHandle<Result<std::path::PathBuf, crate::error::AppError>>,
        std::sync::mpsc::Sender<()>,
    )
    where
        F: FnOnce() -> Result<std::path::PathBuf, crate::error::AppError> + Send + 'static,
    {
        let (gate_tx, gate_rx) = std::sync::mpsc::channel::<()>();
        let join = std::thread::spawn(move || {
            let _ = gate_rx.recv();
            compute()
        });
        (join, gate_tx)
    }

    /// Poll (≤5s) until the ticker sees its job's worker as settled.
    fn await_settle(app: &PacketApp) {
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
        while app
            .clone_job
            .as_ref()
            .is_some_and(|j| !j.join.is_finished())
        {
            assert!(std::time::Instant::now() < deadline, "worker never settled");
            std::thread::sleep(std::time::Duration::from_millis(2));
        }
    }

    /// mkrepo-style source shaped like a VANILLA GitHub repository: init
    /// -b main, LOCAL identity, one committed baseline and DELIBERATELY
    /// no planning/ — the connect pipeline must bootstrap + migrate it
    /// exactly as it does for a hand-typed path into the same tree.
    fn swcl_repo(tag: &str) -> std::path::PathBuf {
        static SEQ: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
        let seq = SEQ.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        let repo = std::env::temp_dir().join(format!("swcl_{tag}_{seq}_{}", std::process::id()));
        std::fs::create_dir_all(&repo).unwrap();
        let git = |args: &[&str]| {
            let status = std::process::Command::new("git")
                .arg("-C")
                .arg(&repo)
                .args(args)
                .status()
                .unwrap();
            assert!(
                status.success(),
                "git {args:?} failed building the {tag} repo"
            );
        };
        git(&["init", "-q", "-b", "main"]);
        git(&["config", "user.name", "SW Clone Test"]);
        git(&["config", "user.email", "sw-clone@example.invalid"]);
        git(&["config", "commit.gpgsign", "false"]);
        std::fs::write(repo.join("README.md"), "# Cloned by SW test\n").unwrap();
        git(&["add", "README.md"]);
        git(&["commit", "-q", "-m", "baseline"]);
        repo
    }

    fn sw_cl_job(
        join: std::thread::JoinHandle<Result<std::path::PathBuf, crate::error::AppError>>,
    ) -> CloneJob {
        CloneJob {
            url_display: "github.com/acme/site".into(),
            repo: "site".into(),
            join,
        }
    }

    #[test]
    fn sw_clone_ticket_one_unsettled_worker_rides_back_next_tick() {
        let (join, gate) =
            gated_worker(|| -> Result<std::path::PathBuf, crate::error::AppError> {
                Err(crate::error::AppError::Other(
                    "abandoned at teardown".into(),
                ))
            });
        let mut app = PacketApp {
            conn_github: "https://github.com/acme/site".into(),
            ..Default::default()
        };
        app.clone_job = Some(sw_cl_job(join));
        let ctx = egui::Context::default();

        // Worker still fetching: the ticket picks the job UP and puts it
        // back (poll rhythm), and NO action is taken meanwhile.
        app.tick(0.016, &ctx);
        assert!(app.clone_job.is_some(), "unsettled worker rides back");
        assert!(
            matches!(app.screen, Screen::Welcome),
            "no navigation while in flight"
        );
        assert!(app.conn_error.is_none(), "no premature error");
        assert_eq!(
            app.conn_github, "https://github.com/acme/site",
            "field preserved in flight"
        );

        // Second ticket while still unsettled: same behaviour repeats.
        app.tick(0.016, &ctx);
        assert!(app.clone_job.is_some(), "second tick: still riding back");

        // Gate release (late) so the thread parks out harmlessly.
        let _ = gate.send(());
    }

    #[test]
    fn sw_clone_ticket_two_success_flows_into_submit_connect_and_connected() {
        // Serialise the ambient-env mutations (git hierarchy + per-user
        // state root) behind the house lock while a REAL connect runs.
        let _shield = crate::core::gitops::test_support::shield("sw-clone-ok");
        let state_home = std::env::temp_dir().join(format!("swcl_state_ok_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&state_home);
        std::fs::create_dir_all(&state_home).unwrap();
        // SAFETY: GIT_HIERARCHY_LOCK is held; only this test touches the
        // per-user state root during the body. The prior value (if any)
        // is restored at the end of the body.
        let prev_state_home = std::env::var_os("PACKET_HOME");
        unsafe {
            std::env::set_var("PACKET_HOME", &state_home);
        }

        let dest = swcl_repo("ok");
        let dest_for_worker = dest.clone();
        let (join, gate) = gated_worker(move || Ok(dest_for_worker));
        let mut app = PacketApp {
            conn_github: "https://github.com/acme/site".into(),
            ..Default::default()
        };
        app.clone_job = Some(sw_cl_job(join));
        let ctx = egui::Context::default();

        app.tick(0.016, &ctx); // unsettled: rides back (proven in test one)
        gate.send(()).unwrap();
        await_settle(&app);

        // TICK TWO: job settled -> join -> refill conn_path -> submit_connect
        // (the single connect authority) -> Connected.
        app.tick(0.016, &ctx);
        assert!(app.clone_job.is_none(), "settled worker is consumed");
        let Screen::Connected(project) = &app.screen else {
            panic!(
                "expected Connected after a successful clone (err={:?})",
                app.conn_error
            );
        };
        let expected_title = dest
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_default();
        assert_eq!(
            project.state.title, expected_title,
            "title from the repo name"
        );
        assert_eq!(
            app.conn_path,
            dest.to_string_lossy().into_owned(),
            "worker output filled conn_path (flow-through)"
        );
        assert!(app.conn_github.is_empty(), "success FORGETS the pasted URL");
        assert!(app.conn_error.is_none());
        assert!(
            dest.join(crate::artifacts::product_docs::INDEX).exists(),
            "the connect pipeline bootstrapped + migrated the cloned repo"
        );

        // Restore the ambient state root (prior value, or absence).
        unsafe {
            match prev_state_home {
                Some(prev) => std::env::set_var("PACKET_HOME", prev),
                None => std::env::remove_var("PACKET_HOME"),
            }
        }
        let _ = std::fs::remove_dir_all(&dest);
        let _ = std::fs::remove_dir_all(&state_home);
    }

    #[test]
    fn sw_clone_ticket_three_failed_clone_raises_git_banner_and_preserves_field() {
        let (join, gate) = gated_worker(|| {
            Err(crate::error::AppError::Git {
                cmd: "clone https://github.com/acme/ghost-repo.git".into(),
                detail: "fatal: repository 'https://github.com/acme/ghost-repo.git/' not found"
                    .into(),
            })
        });
        let mut app = PacketApp {
            conn_github: "https://github.com/acme/ghost-repo".into(),
            ..Default::default()
        };
        app.clone_job = Some(CloneJob {
            url_display: "github.com/acme/ghost-repo".into(),
            repo: "ghost-repo".into(),
            join,
        });
        let ctx = egui::Context::default();

        app.tick(0.016, &ctx);
        gate.send(()).unwrap();
        await_settle(&app);
        app.tick(0.016, &ctx);

        assert!(app.clone_job.is_none());
        assert!(
            matches!(app.screen, Screen::Welcome),
            "failure NEVER navigates"
        );
        assert_eq!(
            app.conn_github, "https://github.com/acme/ghost-repo",
            "field preserved for correction + retry"
        );
        let err = app.conn_error.as_deref().unwrap_or("");
        assert!(
            err.contains("git clone https://github.com/acme/ghost-repo.git failed"),
            "headline line rides in the banner (got: {err})"
        );
        assert!(
            err.contains("not found"),
            "detail line rides in the banner (got: {err})"
        );

        // AC retry: the failed SETTLE left the slot vacant and the field
        // intact, so a pressed Clone dispatches AGAIN — proven with a
        // second gated (network-free) worker occupying the same cycle.
        let (join2, gate2) = gated_worker(|| {
            Err(crate::error::AppError::Git {
                cmd: "clone https://github.com/acme/ghost-repo.git".into(),
                detail: "retry round: still unreachable".into(),
            })
        });
        app.clone_job = Some(CloneJob {
            url_display: "github.com/acme/ghost-repo".into(),
            repo: "ghost-repo".into(),
            join: join2,
        });
        app.tick(0.016, &ctx);
        gate2.send(()).unwrap();
        await_settle(&app);
        app.tick(0.016, &ctx);
        assert!(app.clone_job.is_none(), "second failure also settles");
        assert!(matches!(app.screen, Screen::Welcome), "still on Welcome");
        assert!(
            app.conn_error
                .as_deref()
                .is_some_and(|e| e.contains("retry round")),
            "retry-round banner rendered (got: {:?})",
            app.conn_error
        );
    }

    #[test]
    fn sw_clone_ticket_four_panicked_worker_reports_stopped_unexpectedly() {
        let (join, gate) =
            gated_worker(|| -> Result<std::path::PathBuf, crate::error::AppError> {
                panic!("simulated out-of-memory kill in the fetch")
            });
        let mut app = PacketApp {
            conn_github: "https://github.com/acme/site".into(),
            ..Default::default()
        };
        app.clone_job = Some(sw_cl_job(join));
        let ctx = egui::Context::default();

        app.tick(0.016, &ctx); // rides back while unwound
        gate.send(()).unwrap();
        await_settle(&app);
        app.tick(0.016, &ctx);

        assert!(app.clone_job.is_none());
        assert!(matches!(app.screen, Screen::Welcome));
        assert_eq!(
            app.conn_error.as_deref(),
            Some("The clone worker stopped unexpectedly. Try again.")
        );
    }

    #[test]
    fn sw_begin_clone_blank_is_noop_and_unparsable_urls_spawn_nothing() {
        let mut app = PacketApp {
            conn_github: "   ".into(),
            ..Default::default()
        };
        app.begin_clone_from_field();
        assert!(app.clone_job.is_none(), "blank input spawns nothing");
        assert!(app.conn_error.is_none(), "blank input is a silent no-op");

        let mut app = PacketApp {
            conn_github: "  notaurl  ".into(),
            ..Default::default()
        };
        app.begin_clone_from_field();
        assert!(
            app.clone_job.is_none(),
            "an unparseable URL spawns no worker/process"
        );
        let err = app.conn_error.as_deref().unwrap_or("");
        assert!(
            err.starts_with("Can't clone that URL"),
            "framing line (got: {err})"
        );
        assert!(
            err.contains("https://github.com/octocat/hello-world"),
            "names the canonical shape + example (got: {err})"
        );
        assert_eq!(
            app.conn_github, "  notaurl  ",
            "field untouched for correction"
        );

        // Host mismatch gets its own guidance, still no worker.
        let mut app = PacketApp {
            conn_github: "https://gitee.com/o/r".into(),
            ..Default::default()
        };
        app.begin_clone_from_field();
        assert!(app.clone_job.is_none());
        let err = app.conn_error.as_deref().unwrap_or("");
        assert!(
            err.contains("got gitee.com"),
            "distinct host guidance (got: {err})"
        );
    }

    #[test]
    fn sw_begin_clone_valid_url_dispatches_worker_hermetically() {
        // Proves the dispatch link (parse-success → thread launch → job
        // recorded) WITHOUT escaping the test: the computation seam stands
        // in for the real perform_clone, capturing exactly what a worker
        // would receive — the CANONICAL rebuilt url (never the raw string),
        // with the .git suffix normalized in and segment case preserved.
        use std::sync::{Arc, Mutex};
        let dest =
            std::env::temp_dir().join(format!("swcl_dispatch_{}_widget", std::process::id()));
        let _ = std::fs::remove_dir_all(&dest);
        std::fs::create_dir_all(&dest).unwrap();
        let captured: Arc<Mutex<(String, String)>> = Arc::new(Mutex::default());
        let cap2 = captured.clone();
        let dest_for_seam = dest.clone();
        let mut app = PacketApp {
            conn_github: "  https://github.com/Acme/Widget.git \n".into(),
            ..Default::default()
        };
        app.clone_computation_override = Some(Arc::new(move |source: String, repo: String| {
            *cap2.lock().unwrap() = (source, repo);
            Ok(dest_for_seam.clone())
        }));

        app.begin_clone_from_field(); // trims before parsing (raw had padding)
        let Some(job) = app.clone_job.as_ref() else {
            panic!(
                "valid URL must dispatch a worker (err={:?})",
                app.conn_error
            );
        };
        assert_eq!(
            job.url_display, "github.com/Acme/Widget",
            "badge uses owner/repo"
        );
        assert_eq!(job.repo, "Widget", "segment case preserved as pasted");

        // Duplicate submission WHILE IN-FLIGHT: the occupied slot refuses
        // (defense in depth behind the busy card's input-steal) and the
        // recorded operation stays untouched. The capture below doubles as
        // proof no second worker computation ever ran.
        app.conn_github = "https://github.com/Late/Arrival".into();
        app.begin_clone_from_field();
        let job_after_dup = app
            .clone_job
            .as_ref()
            .expect("duplicate must be refused, slot intact");
        assert_eq!(job_after_dup.url_display, "github.com/Acme/Widget");
        assert!(
            app.conn_error.is_none(),
            "duplicate refused silently (no banner churn)"
        );

        let settled = app.clone_job.take().unwrap().join.join().unwrap().unwrap();
        assert_eq!(settled, dest, "worker result flows back undistorted");
        let (source, repo) = (*captured.lock().unwrap()).clone();
        assert_eq!(
            source, "https://github.com/Acme/Widget.git",
            "worker receives the CANONICAL rebuilt url (not the raw paste)"
        );
        assert_eq!(repo, "Widget");
        let _ = std::fs::remove_dir_all(&dest);
    }

    // ---- Connect-card paint simulation (AC7: in-flight freeze) --------

    /// One paint-frame driver for the connect card, standing in for the
    /// Welcome arm (fresh per-frame flag cells approximated by resets in
    /// the test bodies).
    struct SwCardSim {
        path: String,
        github: String,
        err: Option<String>,
        browse: bool,
        clone_req: bool,
        submitted: bool,
        cloning: Option<(String, String)>,
        /// Field hit-geometry captured inside `paint` every frame — the
        /// reliable ground truth for scripted clicks (painted fills are
        /// theme-dependent; hit geometry is not).
        path_probe: (egui::Id, egui::Rect),
        url_probe: (egui::Id, egui::Rect),
    }

    impl SwCardSim {
        fn frame(&mut self, ctx: &egui::Context, events: Vec<egui::Event>) -> egui::FullOutput {
            let mut out = ctx.run_ui(
                egui::RawInput {
                    screen_rect: Some(egui::Rect::from_min_size(
                        egui::Pos2::ZERO,
                        egui::vec2(1400.0, 900.0),
                    )),
                    events,
                    ..Default::default()
                },
                |ui| {
                    self.submitted = welcome::paint(
                        ui,
                        &mut self.path,
                        &mut self.github,
                        self.err.as_deref(),
                        &mut self.browse,
                        &mut self.clone_req,
                        self.cloning.as_ref().map(|(u, r)| (u.as_str(), r.as_str())),
                        Some(&mut self.path_probe),
                        Some(&mut self.url_probe),
                    );
                },
            );
            out.textures_delta.clear();
            out
        }

        fn click(&mut self, ctx: &egui::Context, pos: egui::Pos2) {
            let btn = egui::PointerButton::Primary;
            let mods = Default::default();
            self.frame(
                ctx,
                vec![
                    egui::Event::PointerMoved(pos),
                    egui::Event::PointerButton {
                        pos,
                        button: btn,
                        pressed: true,
                        modifiers: mods,
                    },
                    egui::Event::PointerButton {
                        pos,
                        button: btn,
                        pressed: false,
                        modifiers: mods,
                    },
                ],
            );
        }
    }

    fn sw_enter_event() -> egui::Event {
        egui::Event::Key {
            key: egui::Key::Enter,
            physical_key: None,
            modifiers: Default::default(),
            pressed: true,
            repeat: false,
        }
    }

    /// The two field hit-rects straight from the in-paint probe (path
    /// first, URL second) — immune to theme-dependent painted fills.
    fn sw_card_fields(sim: &SwCardSim) -> (egui::Rect, egui::Rect) {
        let (path_id, path_rect) = &sim.path_probe;
        let (url_id, url_rect) = &sim.url_probe;
        assert_ne!(*path_id, egui::Id::NULL, "path field was not painted");
        assert_ne!(*url_id, egui::Id::NULL, "url field was not painted");
        for (r, what) in [(path_rect, "path"), (url_rect, "url")] {
            assert!(
                r.height() > 30.0 && r.width() > 100.0,
                "{what} field rect looks degenerate: {r:?}"
            );
        }
        (*path_rect, *url_rect)
    }

    #[test]
    fn sw_clone_card_freezes_in_flight_and_scopes_enter_per_focus() {
        let ctx = egui::Context::default();
        let mut sim = SwCardSim {
            path: String::new(),
            github: String::new(),
            err: None,
            browse: false,
            clone_req: false,
            submitted: false,
            cloning: None,
            path_probe: (egui::Id::NULL, egui::Rect::NOTHING),
            url_probe: (egui::Id::NULL, egui::Rect::NOTHING),
        };

        // Burn the fresh context's placeholder frame, then an idle probe.
        sim.frame(&ctx, vec![]);
        let out = sim.frame(&ctx, vec![]);
        assert!(!sim.submitted, "idle card returns false");
        assert!(!sim.browse && !sim.clone_req, "idle card raises nothing");
        assert!(
            sw_text_pos(&out, "Or paste a GitHub URL").is_some(),
            "row caption painted"
        );
        assert!(sw_text_pos(&out, "Clone").is_some(), "Clone button painted");
        assert!(
            sw_text_pos(&out, "Open workspace").is_some(),
            "existing Open button intact"
        );
        let (path_field, url_field) = sw_card_fields(&sim);

        // Bare window-level Enter with NO field focused: inert (retired
        // global hook).
        sim.browse = false;
        sim.clone_req = false;
        sim.submitted = false;
        sim.frame(&ctx, vec![sw_enter_event()]);
        assert!(
            !sim.submitted && !sim.clone_req && !sim.browse,
            "focus-less Enter changes nothing"
        );

        // URL field focused + Enter: requests a clone, does NOT submit.
        sim.click(&ctx, url_field.center());
        assert_eq!(
            ctx.memory(|m| m.focused()),
            Some(sim.url_probe.0),
            "click focused the URL field"
        );
        sim.clone_req = false;
        sim.submitted = false;
        sim.frame(&ctx, vec![sw_enter_event()]);
        assert!(!sim.submitted, "URL-field Enter must not submit");
        assert!(sim.clone_req, "URL-field Enter requests a clone");

        // Path field focused + Enter: submits only when non-empty.
        sim.click(&ctx, path_field.center());
        assert_eq!(
            ctx.memory(|m| m.focused()),
            Some(sim.path_probe.0),
            "click focused the PATH field"
        );
        sim.clone_req = false;
        sim.submitted = false;
        sim.frame(&ctx, vec![sw_enter_event()]);
        assert!(
            !sim.submitted && !sim.clone_req,
            "empty path + focused Enter is inert (incumbent guard)"
        );
        // Enter already SURRENDED focus (single-line TextEdit behaviour —
        // the retired global shortcut's deliberate casualty): refill and
        // re-focus before the submitting Enter.
        sim.path = "/tmp/somewhere".into();
        sim.click(&ctx, path_field.center());
        sim.submitted = false;
        sim.frame(&ctx, vec![sw_enter_event()]);
        assert!(sim.submitted, "path-field Enter submits the connect");

        // Clone button rect (110x42) is the card's unique tall outline.
        let out = sim.frame(&ctx, vec![]);
        let clone_btn = out
            .shapes
            .iter()
            .find_map(|sl| match &sl.shape {
                egui::Shape::Rect(r) => ((r.rect.size().x - 110.0).abs() < 2.01
                    && (r.rect.size().y - 42.0).abs() < 2.01)
                    .then_some(r.rect),
                _ => None,
            })
            .unwrap_or_else(|| panic!("Clone button rect (110x42) missing"));

        // Empty-URL guard: the control is disabled, so a click on it
        // raises nothing (ticket: mirrors the path field's empty-silence;
        // the parser's complaints belong to non-empty malformations).
        sim.clone_req = false;
        sim.submitted = false;
        sim.click(&ctx, clone_btn.center());
        assert!(
            !sim.clone_req && !sim.submitted,
            "disabled (empty-URL) Clone click raises nothing"
        );

        // Mouse path: once the field holds a URL, clicking Clone activates.
        sim.github = "https://github.com/acme/widget".into();
        sim.frame(&ctx, vec![]); // repaint so the control re-enables
        sim.clone_req = false;
        sim.submitted = false;
        sim.click(&ctx, clone_btn.center());
        assert!(sim.clone_req, "Clone click raises the one-shot request");

        assert!(!sim.submitted, "Clone click does not submit");

        // IN FLIGHT: every input returns false, flags are forced clean,
        // and the status line names the repo.
        sim.cloning = Some(("github.com/acme/widget".into(), "widget".into()));
        sim.clone_req = true; // hostile sticky flag: paint must force it low
        sim.browse = true;
        sim.submitted = false;
        let out = sim.frame(&ctx, vec![sw_enter_event()]);
        assert!(!sim.submitted, "busy card returns false on Enter");
        assert!(!sim.clone_req, "busy card forces the clone flag LOW");
        assert!(!sim.browse, "busy card forces the browse flag LOW");
        assert!(
            sw_text_pos(&out, "Cloning widget from GitHub…").is_some(),
            "in-flight status line painted"
        );
        sim.cloning = None;
    }

    #[test]
    fn sw_clone_ticket_five_unparseable_past_surfaces_readable_banner_and_spawns_nothing() {
        // AC guard: invalid pastes show a readable error near the URL
        // field and NEVER spawn a clone worker.
        let mut app = PacketApp {
            conn_github: "notaurl".into(),
            ..Default::default()
        };
        app.begin_clone_from_field();
        assert!(app.clone_job.is_none(), "no worker spawned for junk");
        let err = app.conn_error.as_deref().unwrap_or("");
        assert!(
            err.starts_with("Can't clone that URL"),
            "readable error surfaced near the field (got: {err})"
        );

        let mut wrong_host = PacketApp {
            conn_github: "https://gitlab.example.com/acme/site".into(),
            ..Default::default()
        };
        wrong_host.begin_clone_from_field();
        assert!(
            wrong_host.clone_job.is_none(),
            "non-GitHub host spawns nothing"
        );
        assert!(wrong_host.conn_error.is_some(), "host diagnostic surfaced");

        let mut blank = PacketApp {
            conn_github: "   ".into(),
            ..Default::default()
        };
        blank.begin_clone_from_field();
        assert!(
            blank.clone_job.is_none() && blank.conn_error.is_none(),
            "blank paste is fully inert"
        );
    }
}
