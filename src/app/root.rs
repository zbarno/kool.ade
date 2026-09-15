//! `PacketApp`: eframe root. Owns the connect screen and the connected
//! screen; drains the turn bus; delegates all pixels to `crate::ui`.

use std::time::{Duration, Instant};

use eframe::{App, Frame};

use crate::app::dialogs::{self, DlgImport, DlgMcp, DlgSettings};
use crate::app::session::{self, Project};
use crate::app::welcome;
use crate::core::turn::{TurnController, TurnEvt, TurnOutcome};
use crate::domain::chatlog::{ChatMessage, ChatRole};
use crate::domain::item::OpenItem;
use crate::domain::user::CurrentUser;
use crate::harness::PiHarness;
use crate::ui::{HeaderAction, Intent, Surface, ToastQueue};

#[cfg(test)]
#[path = "conversation_tests.rs"]
mod conversation_tests;

/// Root of the packet app.
pub struct PacketApp {
    #[cfg(test)]
    task_harness: Option<Box<dyn crate::harness::AiHarness>>,
    screen: Screen,
    dialog: Option<Dialog>,
    toasts: ToastQueue,
    conn_path: String,
    conn_error: Option<String>,
    last_git_refresh: Instant,
    /// Cached routing identity (rebuilt after connect/adoption/settings).
    cached_user: CurrentUser,
    /// D-14 configuration stand-in shown off-project (welcome screen):
    /// an empty map means every category classifies as unowned, so the
    /// pane degrades gracefully until a project connects.
    fallback_stakes: crate::domain::Stakeholders,
    /// Synthesized ownership-gap items for the side pane.
    synth: Vec<OpenItem>,
}

enum Screen {
    Welcome,
    Connected(Project),
}

enum Dialog {
    Import(DlgImport),
    Settings(DlgSettings),
    Mcp(DlgMcp),
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
            #[cfg(test)]
            task_harness: None,
            screen: Screen::Welcome,
            dialog: None,
            toasts,
            conn_path: std::env::current_dir()
                .map(|p| p.to_string_lossy().into_owned())
                .unwrap_or_default(),
            conn_error: None,
            last_git_refresh: Instant::now(),
            cached_user: CurrentUser::new("", Vec::new()),
            fallback_stakes: crate::domain::Stakeholders::default(),
            synth: Vec::new(),
        }
    }
}

// ------------------------------------------------------------------------ tick
impl PacketApp {
    fn tick(&mut self, _dt: f32, ctx: &egui::Context) {
        // Phase 1: drain pending turn events (borrows `self.screen` only).
        let mut outcome: Option<TurnOutcome> = None;
        if let Screen::Connected(project) = &mut self.screen {
            project.task_chats.ensure_loaded(&project.chat_slug);
            if let Some(ctrl) = &project.active_turn {
                for _ in 0..64 {
                    let Some(evt) = ctrl.poll(Duration::ZERO) else {
                        break;
                    };
                    match evt {
                        TurnEvt::Progress(progress) => project.live_progress.update(progress),
                        TurnEvt::Done(o) => {
                            outcome = Some(o);
                            break;
                        }
                    }
                }
            }
        }
        if let Screen::Connected(project) = &mut self.screen {
            let mut finished = None;
            if let Some(ctrl) = &project.active_implementation {
                for _ in 0..64 {
                    match ctrl.poll() {
                        Some(crate::core::implementation::Event::Progress(p)) => {
                            if let Some(ticket) = &project.active_implementation_ticket {
                                project
                                    .activity
                                    .tasks
                                    .entry(ticket.clone())
                                    .or_default()
                                    .update(p);
                            }
                        }
                        Some(crate::core::implementation::Event::Done(result)) => {
                            finished = Some(result);
                            break;
                        }
                        None => break,
                    }
                }
            }
            if let Some(result) = finished {
                project.active_implementation = None;
                if let Some(ticket) = project.active_implementation_ticket.take() {
                    if let Some(progress) = project.activity.tasks.get_mut(&ticket) {
                        progress.telemetry.finished_ms =
                            Some(chrono::Utc::now().timestamp_millis());
                        progress.activity = Some(match &result {
                            Ok(record) => record.status.clone(),
                            Err(_) => "Needs attention".into(),
                        });
                    }
                    project.save_task_activity(&ticket);
                }
                project.refresh_implementations();
                project.last_pr_refresh = None;
                let text = match result {
                    Ok(record) => {
                        project.queue.current_ticket = None;
                        if !record.auto_merge || record.status != "Done" {
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
                        } else {
                            format!(
                                "Implementation verified. Pull request: {}",
                                record.pr_url.unwrap_or_default()
                            )
                        }
                    }
                    Err(error) => {
                        project.queue.running = false;
                        project.queue.last_error = error.clone();
                        format!(
                            "The task needs attention after automatic recovery. Its work is preserved. Open its card for the failure details and Resume action."
                        )
                    }
                };
                if project.queue_lock.is_some() {
                    if let Err(error) = project.queue.save(&project.state.repo_root) {
                        project.queue.running = false;
                        project.queue.last_error = format!("Cannot save queue: {error}");
                    }
                }
                if !project.queue.running {
                    project.queue_lock = None;
                }
                project.activity.pending.push(text.clone());
                project.remember_chat(vec![ChatMessage::new(ChatRole::System, text, None)]);
                project.refresh_git();
            }
        }
        if let Screen::Connected(project) = &mut self.screen {
            if project
                .pr_refresh
                .as_ref()
                .is_some_and(|refresh| refresh.finished())
            {
                project.pr_refresh = None;
                project.refresh_implementations();
            }
            if project.pr_refresh.is_none()
                && project
                    .last_pr_refresh
                    .is_none_or(|last| last.elapsed() >= Duration::from_secs(60))
            {
                let mut states = project
                    .implementation_states
                    .values()
                    .filter(|state| {
                        state.pr_url.is_some() && state.pr_state.as_deref() != Some("MERGED")
                    })
                    .collect::<Vec<_>>();
                states.sort_by_key(|state| &state.pr_check_attempted_at);
                let tickets = states
                    .into_iter()
                    .map(|state| state.ticket.clone())
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
            let slot = std::mem::replace(&mut self.screen, Screen::Welcome);
            if let Screen::Connected(mut project) = slot {
                self.adopt_turn(&mut project, o);
                self.screen = Screen::Connected(project);
            }
        }
        // Phase 3: periodic git refresh + cache re-derivation.
        if self.last_git_refresh.elapsed() > Duration::from_secs(3) {
            let caches = match &mut self.screen {
                Screen::Connected(p) => {
                    p.refresh_git();
                    p.refresh_implementations();
                    p.task_documents = crate::artifacts::task_docs::load_latest(
                        &p.state.repo_root,
                        &p.state.workflow,
                    );
                    Self::derive_caches(p)
                }
                Screen::Welcome => return,
            };
            (self.cached_user, self.synth) = caches;
            self.last_git_refresh = Instant::now();
        }
        if let Screen::Connected(project) = &mut self.screen {
            if project
                .activity
                .last_save
                .is_none_or(|last| last.elapsed() >= Duration::from_secs(2))
            {
                if let Some(ticket) = project.active_implementation_ticket.clone() {
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
            if project.active_implementation.is_some()
                && project.activity.pending.is_empty()
                && project
                    .activity
                    .last_update
                    .is_some_and(|last| last.elapsed() >= Duration::from_secs(120))
            {
                project.activity.pending.push("The worker is still running. No completion is confirmed; review the current task states and help the user with the next eligible planning decision without inventing progress.".into());
            }
            if project.active_turn.is_none()
                && project.activity.manager.is_none()
                && !project.activity.pending.is_empty()
                && project
                    .activity
                    .last_update
                    .is_none_or(|last| last.elapsed() >= Duration::from_secs(30))
            {
                let events = std::mem::take(&mut project.activity.pending);
                project.activity.manager = Some(super::manager::Manager::start(project, &events));
                project.activity.last_update = Some(Instant::now());
                project.live_progress = crate::harness::LiveProgress {
                    activity: Some("Reviewing project progress…".into()),
                    ..Default::default()
                };
            }
        }
        self.advance_auto_queue();
        self.advance_reconciliation();
        self.advance_investigation();
        let period = match &self.screen {
            Screen::Connected(p)
                if (p.active_turn.is_some()
                    || p.active_implementation.is_some()
                    || p.reconciliation.is_some()
                    || p.investigation.is_some()
                    || p.activity.manager.is_some()) =>
            {
                Duration::from_millis(120)
            }
            _ => Duration::from_millis(800),
        };
        ctx.request_repaint_after(period);
    }

    fn advance_auto_queue(&mut self) {
        let next = if let Screen::Connected(project) = &mut self.screen {
            if !project.queue.auto_mode
                || !project.queue.running
                || project.active_turn.is_some()
                || project.active_implementation.is_some()
            {
                return;
            }
            if project.queue_lock.is_none() {
                match crate::core::implementation_queue::Queue::acquire(&project.state.repo_root) {
                    Ok(lock) => project.queue_lock = Some(lock),
                    Err(error) => {
                        project.queue.running = false;
                        project.queue.last_error = error.to_string();
                        return;
                    }
                }
            }
            let pending = project.queue.current_ticket.clone().filter(|ticket| {
                !project
                    .implementation_states
                    .get(ticket)
                    .is_some_and(|state| state.status == "Done")
            });
            let choice = pending.map(|ticket| Ok(Some(ticket))).unwrap_or_else(|| {
                crate::core::implementation_queue::next_ticket(
                    &project.task_documents,
                    &project.implementation_states,
                )
            });
            match choice {
                Ok(Some(ticket)) => Some(ticket),
                Ok(None) => {
                    project.queue.running = false;
                    project.queue.current_ticket = None;
                    project.queue.last_error.clear();
                    if let Err(error) = project.queue.save(&project.state.repo_root) {
                        project.queue.last_error = error.to_string();
                    }
                    project.queue_lock = None;
                    None
                }
                Err(error) => {
                    project.queue.last_error = error;
                    None
                }
            }
        } else {
            None
        };
        if let Some(ticket) = next {
            self.implement_task(ticket);
        }
    }

    fn advance_reconciliation(&mut self) {
        let Screen::Connected(project) = &mut self.screen else {
            return;
        };
        if let Some(controller) = &project.reconciliation {
            if let Some(result) = controller.poll() {
                let feature_id = controller.feature_id.clone();
                project.reconciliation = None;
                match result {
                    Ok((state, message)) => {
                        project.state = state;
                        project.task_documents = crate::artifacts::task_docs::load_latest(
                            &project.state.repo_root,
                            &project.state.workflow,
                        );
                        project.refresh_git();
                        project.reconciliation_error = None;
                        project
                            .activity
                            .pending
                            .push(format!("Reconciled {feature_id}: {message}"));
                        self.toasts.success(format!("Reconciled {feature_id}"));
                    }
                    Err(error) => {
                        if let Ok(current) =
                            crate::core::state::PlannerState::load(&project.state.repo_root)
                        {
                            project.state = current;
                        }
                        project.reconciliation_error = Some(error.to_string());
                        project.activity.pending.push(format!(
                            "Reconciliation of {feature_id} needs attention: {error}"
                        ));
                        self.toasts
                            .warning(format!("Reconciliation needs attention: {error}"));
                    }
                }
            }
        }
        if project.reconciliation.is_some()
            || project.active_turn.is_some()
            || project.active_implementation.is_some()
            || project.queue.running
        {
            return;
        }
        let Some((feature_id, _)) = &project.state.active_feature else {
            return;
        };
        if project.reconciliation_attempted.contains(feature_id) {
            return;
        }
        match crate::core::reconciliation::candidate(&project.state) {
            Ok(Some(candidate)) => {
                project
                    .reconciliation_attempted
                    .insert(candidate.feature_id.clone());
                project.activity.pending.push(format!("All tasks for {} have merged; checking actual implementation against the approved feature.", candidate.feature_id));
                project.reconciliation = Some(crate::core::reconciliation::Controller::start(
                    project.state.clone(),
                    candidate,
                ));
            }
            Ok(None) => {}
            Err(error) => {
                project.reconciliation_attempted.insert(feature_id.clone());
                project.reconciliation_error = Some(error.to_string());
                project.activity.pending.push(format!(
                    "Reconciliation of {feature_id} needs attention: {error}"
                ));
            }
        }
    }

    fn advance_investigation(&mut self) {
        let Screen::Connected(project) = &mut self.screen else {
            return;
        };
        if let Some(controller) = &project.investigation {
            let item_id = controller.item_id.clone();
            let mut finished = None;
            for _ in 0..64 {
                match controller.poll() {
                    Some(crate::core::investigation::Event::Progress(update)) => {
                        project
                            .activity
                            .tasks
                            .entry(item_id.clone())
                            .or_default()
                            .update(update);
                    }
                    Some(crate::core::investigation::Event::Done(result)) => {
                        finished = Some(result);
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
                match result {
                    Ok((state, message)) => {
                        project.state = state;
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
                        project.save_task_activity(&item_id);
                    }
                }
            }
        }
        if project.investigation.is_some()
            || project.active_turn.is_some()
            || project.reconciliation.is_some()
        {
            return;
        }
        let next = project
            .state
            .items
            .iter()
            .filter(|item| {
                item.authority == crate::domain::Authority::Agent
                    && !project.investigation_attempted.contains(&item.id)
            })
            .min_by_key(|item| (item.priority.rank(), &item.id));
        if let Some(item) = next {
            let item_id = item.id.clone();
            project.investigation_attempted.insert(item_id.clone());
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
            project.investigation = Some(crate::core::investigation::Controller::start(
                project.state.clone(),
                item_id,
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

    fn adopt_turn(&mut self, project: &mut Project, outcome: TurnOutcome) {
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
                project.state = state;
                project.task_documents = crate::artifacts::task_docs::load_latest(
                    &project.state.repo_root,
                    &project.state.workflow,
                );
                if project.task_chats.active.is_none() {
                    project.next_question_id = normalized.next_question_id.clone();
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
                if project.state.workflow.task_batches.len() > previous_batches {
                    if let Some(batch) = project.state.workflow.task_batches.last() {
                        chat.push(ChatMessage::new(ChatRole::System, format!("Created {} detailed task stories in {}. Open the Task stories tab to review them.", batch.count, batch.directory), None));
                    }
                }
                project.remember_turn_chat(chat);
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
    }

    fn submit_task_reply(&mut self, key: &str) {
        let Screen::Connected(project) = &mut self.screen else {
            return;
        };
        if project.active_turn.is_some() || project.reconciliation.is_some() {
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
        if let Some(controller) = project.investigation.take() {
            project.investigation_attempted.remove(&controller.item_id);
            controller.cancel();
        }
        project.activity.manager = None;
        let inputs = crate::core::turn::TurnInputs {
            state: project.state.clone(),
            user_message: text,
            recent_chat,
            purpose: crate::core::workflow::TurnPurpose::Interview,
        };
        project.task_chats.drafts.remove(key);
        project.task_chats.active = Some(key.into());
        #[cfg(test)]
        let harness = self
            .task_harness
            .take()
            .unwrap_or_else(|| Box::new(PiHarness));
        #[cfg(not(test))]
        let harness = Box::new(PiHarness);
        project.active_turn = Some(std::sync::Arc::new(TurnController::start_scoped(
            inputs,
            harness,
            Some(key.into()),
        )));
        project.live_progress = crate::harness::LiveProgress::default();
    }

    // ---------------------------------------------------------------- actions
    fn submit_connect(&mut self) {
        if self.conn_path.trim().is_empty() {
            return;
        }
        match welcome::attempt_connect(&self.conn_path) {
            Ok(mut project) => {
                self.refresh_derived(&project);
                project.remember_chat(vec![session::welcome_message(&project.state.title)]);
                self.conn_error = None;
                let title = project.state.title.clone();
                self.screen = Screen::Connected(project);
                self.toasts.success(format!("Connected to {title}"));
            }
            Err(e) => {
                self.conn_error = Some(format!("{}\n{}", e.headline(), e.detail()));
            }
        }
    }

    fn start_turn(&mut self, text: &str) {
        let purpose = match &self.screen {
            Screen::Connected(p)
                if p.active_implementation.is_none()
                    && p.state.workflow.ready(p.state.planning_contract())
                    && crate::core::workflow::confirms_generation(text) =>
            {
                crate::core::workflow::TurnPurpose::GenerateTasks
            }
            _ => crate::core::workflow::TurnPurpose::Interview,
        };
        self.start_turn_with_purpose(text, purpose);
    }

    fn start_turn_with_purpose(&mut self, text: &str, purpose: crate::core::workflow::TurnPurpose) {
        let Screen::Connected(project) = &mut self.screen else {
            return;
        };
        if project.reconciliation.is_some() {
            project.draft = text.to_string();
            self.toasts
                .info("Reconciliation is checking merged implementation; your draft is preserved.");
            return;
        }
        if let Some(controller) = project.investigation.take() {
            project.investigation_attempted.remove(&controller.item_id);
            controller.cancel();
        }
        if project.active_turn.is_some()
            || (project.active_implementation.is_some()
                && purpose == crate::core::workflow::TurnPurpose::GenerateTasks)
        {
            return;
        }
        project.activity.manager = None;
        let recent = project.recent_chat_tuples(6, 1200);
        project.remember_chat(vec![ChatMessage::new(ChatRole::User, text, None)]);
        let inputs = crate::core::turn::TurnInputs {
            state: project.state.clone(),
            user_message: format!(
                "{text}\n\n[Application project context: active task worker={:?}; auto queue running={}; task count={}; recent task states={:?}. Continue managing the project and engaging this user while the isolated worker handles implementation. Do not claim to steer or stop a worker through prose; task controls manage that. Planning answers may update the specification normally.]",
                project.active_implementation_ticket,
                project.queue.running,
                project.implementation_states.len(),
                project
                    .implementation_states
                    .iter()
                    .rev()
                    .take(5)
                    .map(|(ticket, state)| (ticket, &state.status))
                    .collect::<Vec<_>>()
            ),
            recent_chat: recent,
            purpose,
        };
        let ctrl = TurnController::start(inputs, Box::new(PiHarness));
        project.active_turn = Some(std::sync::Arc::new(ctrl));
        project.live_progress = crate::harness::LiveProgress {
            activity: Some("Starting planner…".into()),
            ..Default::default()
        };
    }

    fn disconnect(&mut self) {
        if let Screen::Connected(p) = &mut self.screen {
            if p.active_turn.is_some() {
                if let Some(ctrl) = &p.active_implementation {
                    ctrl.request_cancel();
                }
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
    fn resolved_items(&self) -> &[OpenItem] {
        match &self.screen {
            Screen::Connected(p) => &p.state.resolved_items,
            _ => &[],
        }
    }
    fn task_draft(&mut self, key: &str) -> Option<&mut String> {
        match &mut self.screen {
            Screen::Connected(p) => Some(p.task_chats.drafts.entry(key.into()).or_default()),
            _ => None,
        }
    }
    fn send_task_reply(&mut self, key: &str) {
        self.submit_task_reply(key);
    }
    fn task_chat_active(&self, key: &str) -> bool {
        matches!(&self.screen, Screen::Connected(p) if p.active_turn.is_some() && p.task_chats.active.as_deref() == Some(key))
    }
    fn task_chat_error(&self) -> Option<&str> {
        match &self.screen {
            Screen::Connected(p) => p.task_chats.error.as_deref(),
            _ => None,
        }
    }
    fn task_reply_busy(&self) -> bool {
        matches!(&self.screen, Screen::Connected(p) if p.active_turn.is_some() || p.reconciliation.is_some())
    }
    fn cancel_task_reply(&mut self, key: &str) {
        if let Screen::Connected(p) = &self.screen {
            if p.task_chats.active.as_deref() == Some(key) {
                if let Some(turn) = &p.active_turn {
                    turn.request_cancel();
                }
            }
        }
    }
    fn retry_task_chat_save(&mut self) {
        if let Screen::Connected(p) = &mut self.screen {
            p.task_chats.retry_save(&p.chat_slug);
        }
    }

    fn is_busy(&self) -> bool {
        matches!(&self.screen, Screen::Connected(p) if p.active_turn.is_some() || p.active_implementation.is_some())
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
    fn cancel_task(&mut self) {
        if let Screen::Connected(p) = &mut self.screen {
            p.queue.running = false;
            if p.queue_lock.is_some() {
                if let Err(error) = p.queue.save(&p.state.repo_root) {
                    p.queue.last_error = error.to_string();
                }
            }
            if let Some(ctrl) = &p.active_implementation {
                ctrl.request_cancel();
            }
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
                    && p.active_implementation.is_none()
                    && p.state.workflow.ready(p.state.planning_contract()) =>
            {
                p.state.workflow.brief.as_ref()
            }
            _ => None,
        }
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
    fn implementation_active(&self, ticket: &str) -> bool {
        matches!(&self.screen, Screen::Connected(p) if p.active_implementation_ticket.as_deref() == Some(ticket))
    }
    fn auto_mode(&self) -> bool {
        matches!(&self.screen, Screen::Connected(p) if p.queue.auto_mode)
    }
    fn queue_status(&self) -> &str {
        match &self.screen {
            Screen::Connected(p) if !p.queue.last_error.is_empty() => &p.queue.last_error,
            Screen::Connected(p) if p.queue.running => {
                "Auto queue running — verified tasks merge into the default branch"
            }
            _ => "",
        }
    }
    fn set_auto_mode(&mut self, enabled: bool) {
        if let Screen::Connected(p) = &mut self.screen {
            let temporary_lock = if p.queue_lock.is_none() {
                match crate::core::implementation_queue::Queue::acquire(&p.state.repo_root) {
                    Ok(lock) => Some(lock),
                    Err(error) => {
                        p.queue.last_error = error.to_string();
                        return;
                    }
                }
            } else {
                None
            };
            p.queue.auto_mode = enabled;
            if !enabled {
                p.queue.running = false;
            }
            if let Err(error) = p.queue.save(&p.state.repo_root) {
                p.queue.last_error = error.to_string();
            }
            drop(temporary_lock);
            if !p.queue.running && p.active_implementation.is_none() {
                p.queue_lock = None;
            }
        }
    }
    fn implement_task(&mut self, ticket: String) {
        if self.is_busy() {
            return;
        }
        if let Screen::Connected(p) = &mut self.screen {
            if p.implementation_states
                .get(&ticket)
                .is_some_and(|state| state.pr_url.is_some() || state.status == "Done")
            {
                return;
            }
            if !p
                .task_documents
                .iter()
                .any(|d| d.path == ticket && !d.path.ends_with("/README.md"))
            {
                return;
            }
            if let Some(doc) = p.task_documents.iter().find(|d| d.path == ticket) {
                if let Some(id) = doc
                    .text
                    .lines()
                    .find_map(|line| line.strip_prefix("Feature ID: "))
                {
                    if !crate::core::workflow::feature_approved(
                        &p.state.repo_root,
                        &p.state.workflow,
                        id,
                    ) {
                        p.queue.last_error =
                            format!("{id} needs explicit approval before implementation");
                        return;
                    }
                }
            }
            let target_repo =
                match crate::core::implementation::target_repository(&p.state.repo_root, &ticket) {
                    Ok(target) => target,
                    Err(error) => {
                        p.queue.last_error = format!("Cannot start {ticket}: {error}");
                        return;
                    }
                };
            if p.queue.auto_mode {
                let selected = p
                    .task_documents
                    .iter()
                    .find(|doc| doc.path == ticket)
                    .unwrap();
                if let Err(error) = crate::core::implementation_queue::next_ticket(
                    std::slice::from_ref(selected),
                    &p.implementation_states,
                ) {
                    p.queue.last_error = error;
                    return;
                }
                if p.queue_lock.is_none() {
                    match crate::core::implementation_queue::Queue::acquire(&p.state.repo_root) {
                        Ok(lock) => p.queue_lock = Some(lock),
                        Err(error) => {
                            p.queue.last_error = error.to_string();
                            return;
                        }
                    }
                }
                p.queue.running = true;
                p.queue.current_ticket = Some(ticket.clone());
                p.queue.last_error.clear();
                if let Err(error) = p.queue.save(&p.state.repo_root) {
                    p.queue.running = false;
                    p.queue.last_error = error.to_string();
                    p.queue_lock = None;
                    return;
                }
            }
            p.activity.pending.push(format!("Assigned task {ticket} to an implementation worker. Verification and integration are managed by the queue."));
            p.remember_chat(vec![ChatMessage::new(
                ChatRole::System,
                format!(
                    "Assigned {ticket}; the worker will verify and {}.",
                    if p.queue.auto_mode {
                        "merge atomically into the default branch, then continue the queue"
                    } else {
                        "create a pull request"
                    }
                ),
                None,
            )]);
            p.activity.tasks.entry(ticket.clone()).or_default().activity =
                Some("Starting implementation…".into());
            p.activity
                .tasks
                .entry(ticket.clone())
                .or_default()
                .telemetry = crate::harness::ActivityTelemetry {
                started_ms: Some(chrono::Utc::now().timestamp_millis()),
                ..Default::default()
            };
            p.active_implementation_ticket = Some(ticket.clone());
            p.active_implementation = Some(crate::core::implementation::Controller::start_project(
                p.state.repo_root.clone(),
                target_repo,
                ticket,
                p.queue.auto_mode,
            ));
        }
    }

    fn task_documents(&self) -> &[crate::artifacts::task_docs::TaskDocument] {
        match &self.screen {
            Screen::Connected(p) => &p.task_documents,
            _ => &[],
        }
    }

    fn items(&self) -> &[OpenItem] {
        match &self.screen {
            Screen::Connected(p) => p.state.items.as_slice(),
            Screen::Welcome => &[],
        }
    }

    fn synthetic_items(&self) -> &[OpenItem] {
        self.synth.as_slice()
    }

    fn items_len(&self) -> usize {
        match &self.screen {
            Screen::Connected(p) => p.state.items.len(),
            Screen::Welcome => 0,
        }
    }

    fn current_user(&self) -> &CurrentUser {
        &self.cached_user
    }

    fn stakeholders(&self) -> &crate::domain::Stakeholders {
        match &self.screen {
            Screen::Connected(p) => &p.state.config.stakeholders,
            Screen::Welcome => &self.fallback_stakes,
        }
    }

    fn next_question_id(&self) -> Option<&str> {
        match &self.screen {
            Screen::Connected(p) => p.next_question_id.as_deref(),
            Screen::Welcome => None,
        }
    }

    fn approve_review_item(&mut self, id: &str) {
        let Screen::Connected(project) = &mut self.screen else {
            return;
        };
        if project.active_turn.is_some() {
            self.toasts
                .warning("Finish the active planning turn before approving this review.");
            return;
        }
        match crate::core::board_actions::approve_review(&mut project.state, id) {
            Ok(_) => {
                project.next_question_id = None;
                project.activity.pending.push(format!(
                    "Approved review {id}; the feature decision and board are updated."
                ));
                self.toasts.success(format!("Approved review {id}"));
            }
            Err(error) => {
                if let Ok(current) =
                    crate::core::state::PlannerState::load(&project.state.repo_root)
                {
                    project.state = current;
                }
                self.toasts
                    .danger(format!("Could not approve {id}: {error}"));
            }
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

    fn active_feature(&self) -> Option<(&str, &str)> {
        match &self.screen {
            Screen::Connected(p) => p
                .state
                .active_feature
                .as_ref()
                .map(|(id, body)| (id.as_str(), body.as_str())),
            Screen::Welcome => None,
        }
    }
    fn active_feature_approved(&self) -> bool {
        match &self.screen {
            Screen::Connected(p) => p.state.active_feature.as_ref().is_some_and(|(id, _)| {
                crate::core::workflow::feature_approved(&p.state.repo_root, &p.state.workflow, id)
            }),
            Screen::Welcome => false,
        }
    }
    fn approve_active_feature(&mut self) {
        if let Screen::Connected(p) = &mut self.screen {
            let Some((id, _)) = p.state.active_feature.as_ref() else {
                return;
            };
            let id = id.clone();
            match crate::core::workflow::approve_feature(
                &p.state.repo_root,
                &mut p.state.workflow,
                &id,
            ) {
                Ok(_) => {
                    p.refresh_git();
                    self.toasts
                        .success(format!("Approved {id} for implementation"));
                }
                Err(error) => self
                    .toasts
                    .danger(format!("Cannot approve feature: {error}")),
            }
        }
    }
    fn task_story_preview(&self) -> Option<&str> {
        match &self.screen {
            Screen::Connected(p) => p
                .task_documents
                .iter()
                .find(|doc| !doc.path.ends_with("/README.md"))
                .map(|doc| doc.text.as_str()),
            Screen::Welcome => None,
        }
    }
    fn spec_words(&self) -> usize {
        self.spec_text().split_whitespace().count()
    }

    fn toasts(&mut self) -> &mut ToastQueue {
        &mut self.toasts
    }

    fn on_intent(&mut self, intent: &Intent) {
        if intent.generate_tasks {
            if self.task_offer().is_some() {
                self.start_turn_with_purpose(
                    "Yes, proceed to task generation for the reviewed specification.",
                    crate::core::workflow::TurnPurpose::GenerateTasks,
                );
            }
            return;
        }
        if intent.cancel {
            if let Screen::Connected(p) = &mut self.screen {
                p.activity.manager = None;
                if let Some(ctrl) = &p.active_turn {
                    ctrl.request_cancel();
                    self.toasts.warning("Cancellation requested…");
                }
            }
            return;
        }
        if intent.send {
            let taken = match &mut self.screen {
                Screen::Connected(p) => std::mem::take(&mut p.draft),
                Screen::Welcome => return,
            };
            let text = taken.trim();
            if !text.is_empty() {
                self.start_turn(text);
            }
        }
    }

    fn on_header_action(&mut self, action: HeaderAction) {
        match action {
            HeaderAction::Refresh => {
                if let Screen::Connected(p) = &mut self.screen {
                    p.last_pr_refresh = None;
                    p.refresh_git();
                    p.refresh_implementations();
                    p.task_documents = crate::artifacts::task_docs::load_latest(
                        &p.state.repo_root,
                        &p.state.workflow,
                    );
                    self.toasts.info("Git state refreshed");
                }
            }
            HeaderAction::Import => {
                self.dialog = Some(Dialog::Import(DlgImport::new()));
            }
            HeaderAction::Stakeholders => {
                if let Screen::Connected(p) = &self.screen {
                    self.dialog = Some(Dialog::Settings(DlgSettings::from_project(p)));
                }
            }
            HeaderAction::McpServers => {
                // No busy-guard, matching the adjacent Import/Stakeholders
                // arms: the rename-swap keeps any in-flight turn observing a
                // whole old or whole new file (NFR-2).
                if let Screen::Connected(p) = &self.screen {
                    self.dialog = Some(Dialog::Mcp(DlgMcp::from_project(p)));
                }
            }
            HeaderAction::CopySpec => self.copy_spec_to_clipboard(),
            HeaderAction::Disconnect => self.disconnect(),
        }
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
                egui::CentralPanel::default().show(ui, |ui| {
                    ui.vertical_centered(|ui| {
                        ui.add_space((ui.available_height() * 0.30).max(20.0));
                        crate::ui::theme::card_frame().show(ui, |ui| {
                            *slot.borrow_mut() =
                                welcome::paint(ui, &mut self.conn_path, self.conn_error.as_deref());
                        });
                    });
                });
                if *slot.borrow() {
                    self.submit_connect();
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

        self.toasts.show(&ui.ctx());
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
        let root = std::env::temp_dir().join("packet-board-ui-fixture-nonexistent");
        let docs = ["First task", "Review task", "Merged task"]
            .iter()
            .enumerate()
            .map(|(i, title)| crate::artifacts::task_docs::TaskDocument {
                path: format!("planning/tasks/fixture/{:03}-task.md", i + 1),
                title: title.to_string(),
                text: format!("# {title}\n\nUnique story detail {i}"),
            })
            .collect::<Vec<_>>();
        let mut states = std::collections::BTreeMap::new();
        for (i, pr_state) in [(1, "OPEN"), (2, "MERGED")] {
            let record = crate::core::implementation::Implementation {
                ticket: docs[i].path.clone(),
                ticket_text: docs[i].text.clone(),
                approved_specification: None,
                approved_product_context: None,
                completed_dependency_context: None,
                branch: "packet/fixture".into(),
                base: "main".into(),
                base_commit: "fixture".into(),
                worktree: root.join("worktree"),
                status: if i == 1 { "PR created" } else { "Done" }.into(),
                detail: String::new(),
                pr_url: Some(format!("https://github.com/fixture/repo/pull/{i}")),
                verified_head: Some("fixture".into()),
                auto_merge: false,
                merged_commit: None,
                pr_state: Some(pr_state.into()),
                pr_checked_at: None,
                pr_check_attempted_at: None,
                pr_check_error: None,
            };
            states.insert(record.ticket.clone(), record);
        }
        PacketApp {
            screen: Screen::Connected(Project {
                task_chats: Default::default(),
                activity: Default::default(),
                state: crate::core::state::PlannerState::load(&root).unwrap(),
                chat_slug: "unused".into(),
                chat: Vec::new(),
                draft: String::new(),
                queue: Default::default(),
                queue_lock: None,
                active_implementation: None,
                active_implementation_ticket: None,
                implementation_states: states,
                pr_refresh: None,
                reconciliation: None,
                reconciliation_attempted: Default::default(),
                reconciliation_error: None,
                investigation: None,
                investigation_attempted: Default::default(),
                last_pr_refresh: None,
                active_turn: None,
                live_progress: Default::default(),
                next_question_id: None,
                git: Default::default(),
                task_documents: docs,
            }),
            ..Default::default()
        }
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
            if let egui::Shape::Text(text) = &shape.shape {
                if text.galley.text() == needle {
                    return Some(text.pos + text.galley.mesh_bounds.center().to_vec2());
                }
            }
            None
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
    fn narrow_workspace_keeps_board_visible_and_collapses_conversation() {
        let mut app = fixture();
        let ctx = egui::Context::default();
        let size = egui::vec2(360.0, 480.0);

        let expanded = frame_at(&mut app, &ctx, vec![], size);
        let board = text_position(&expanded, "Board  3").expect("board tab should remain visible");
        assert!(board.x < size.x && board.y < size.y);
        assert!(text_position(&expanded, "▾  Conversation").is_some());
        assert!(text_position(&expanded, "Project manager").is_some());

        let collapsed = click_text_at(&mut app, &ctx, "▾  Conversation", size);
        assert!(text_position(&collapsed, "▸  Conversation").is_some());
        let collapsed_board =
            text_position(&collapsed, "Board  3").expect("board should remain after collapse");
        assert!(collapsed_board.y < board.y);
    }

    #[test]
    fn board_overview_collapses_status_above_kanban() {
        let mut app = fixture();
        let ctx = egui::Context::default();
        let expanded = frame(&mut app, &ctx, vec![]);
        assert!(
            text_position(
                &expanded,
                "Auto mode — merge verified tasks and continue the queue"
            )
            .is_some()
        );

        let mut collapsed = click_text(&mut app, &ctx, "Board overview");
        for _ in 0..30 {
            collapsed = frame(&mut app, &ctx, vec![]);
        }
        assert!(
            text_position(
                &collapsed,
                "Auto mode — merge verified tasks and continue the queue"
            )
            .is_none()
        );
        assert!(text_position(&collapsed, "To do · 1").is_some());
    }

    #[test]
    fn document_switcher_displays_active_feature_product_and_task_story() {
        let mut app = fixture();
        if let Screen::Connected(project) = &mut app.screen {
            project.state.spec_text = Some("# Product\n\nProduct behavior marker".into());
            project.state.active_feature = Some((
                "CHG-001".into(),
                "# Feature\n\nFeature proposal marker".into(),
            ));
        }
        let ctx = egui::Context::default();
        ctx.data_mut(|data| data.insert_temp(egui::Id::new("packet_document_tab"), false));
        frame(&mut app, &ctx, vec![]);
        let feature = frame(&mut app, &ctx, vec![]);
        assert!(text_position(&feature, "Feature proposal marker").is_some());
        assert!(text_position(&feature, "Product behavior marker").is_none());
        let product = click_text(&mut app, &ctx, "Product Specification");
        assert!(text_position(&product, "Product behavior marker").is_some());
        assert!(text_position(&product, "Feature proposal marker").is_none());
        let story = click_text(&mut app, &ctx, "Task Stories");
        assert!(text_position(&story, "Unique story detail 0").is_some());
        let feature_again = click_text(&mut app, &ctx, "Active Feature");
        assert!(text_position(&feature_again, "Feature proposal marker").is_some());
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
            if let egui::Shape::Rect(rect) = &shape.shape {
                if rect.corner_radius.nw == 8 && rect.fill == crate::ui::theme::BG {
                    assert!(
                        rect.rect.right() <= 1773.0,
                        "Board column overflows the main panel: {:?}",
                        rect.rect
                    );
                }
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
            Some("planning/tasks/fixture/002-task.md")
        );
        assert!(
            text_position(&output, "Unique story detail 1").is_some(),
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
        app.implement_task("planning/tasks/fixture/002-task.md".into());
        assert!(
            !app.is_busy(),
            "published tasks must not start another agent"
        );
    }
    #[test]
    fn live_card_opens_full_activity_and_returns_to_item_details() {
        let mut app = fixture();
        let ticket = "planning/tasks/fixture/001-task.md".to_owned();
        if let Screen::Connected(p) = &mut app.screen {
            p.active_implementation_ticket = Some(ticket.clone());
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
        assert!(text_position(&output, "LIVE ACTIVITY").is_some());
        assert!(text_position(&output, "Checking the permissions test results").is_some());
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
        click(&mut app, text_position(&output, "First task").unwrap());
        frame(&mut app, &ctx, vec![]);
        let output = frame(&mut app, &ctx, vec![]);
        // Select the modal's activity action, not the card under its backdrop.
        let pos = output
            .shapes
            .iter()
            .rev()
            .find_map(|shape| match &shape.shape {
                egui::Shape::Text(t) if t.galley.text() == "View all activity" => {
                    Some(t.pos + t.galley.mesh_bounds.center().to_vec2())
                }
                _ => None,
            })
            .unwrap();
        click(&mut app, pos);
        frame(&mut app, &ctx, vec![]);
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
        frame(&mut app, &ctx, vec![]);
        let output = frame(&mut app, &ctx, vec![]);
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
        assert!(text_position(&output, "For you").is_some());
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
        assert!(text_position(&output, "Owner: All").is_some());
        assert!(text_position(&output, "Task conversation").is_some());
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
            crate::artifacts::items_io::serialize(&[item.clone()]),
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
            "To do · 2",
            "In progress · 1",
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
        assert!(text_position(&output, "In progress · 1").is_some());
        assert!(text_position(&output, "Reading search source").is_some());
        let details = click_text(&mut app, &ctx, &item.question);
        assert!(text_position(&details, "Agent investigation").is_some());
        assert!(text_position(&details, "Worker thoughts").is_some());
        assert!(
            app.live_progress().is_none(),
            "Item worker output must stay out of main chat"
        );
    }

    #[test]
    fn auto_queue_cannot_start_task_from_unapproved_feature() {
        let mut app = fixture();
        let ticket = "planning/tasks/fixture/001-task.md";
        if let Screen::Connected(project) = &mut app.screen {
            assert!(project.queue.auto_mode);
            project.task_documents[0]
                .text
                .push_str("\nFeature ID: CHG-001\n");
        }
        app.implement_task(ticket.into());
        let Screen::Connected(project) = &app.screen else {
            panic!("disconnected");
        };
        assert!(project.active_implementation.is_none());
        assert!(!project.queue.running);
        assert!(project.queue.last_error.contains("needs explicit approval"));
    }

    #[test]
    fn auto_queue_runs_two_tasks_through_pi_and_merges_without_prs() {
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
        let root = std::env::temp_dir().join(format!(
            "packet-auto-e2e-{}",
            chrono::Utc::now().timestamp_nanos_opt().unwrap()
        ));
        let repo = root.join("repo");
        let remote = root.join("remote.git");
        std::fs::create_dir_all(repo.join("planning/tasks/fixture")).unwrap();
        std::fs::create_dir_all(repo.join(".planner")).unwrap();
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
        let docs = (1..=2).map(|number| crate::artifacts::task_docs::TaskDocument {
            path: format!("planning/tasks/fixture/{number:03}-task.md"), title: format!("Task {number}"),
            text: format!("# Task {number}\n\n## Dependencies\n{}\n\n## Acceptance criteria\n- File exists.\n", if number == 2 { "- [Task 001](001-task.md) must be complete." } else { "None." }),
        }).collect::<Vec<_>>();
        for doc in &docs {
            std::fs::write(repo.join(&doc.path), &doc.text).unwrap();
        }
        std::fs::write(repo.join(".planner/workflow.json"), serde_json::json!({"brief":null,"reviewedSpecification":null,"taskBatches":[{"feature":"fixture","directory":"planning/tasks/fixture","count":2}]}).to_string()).unwrap();
        git(&repo, &["add", "."]);
        git(&repo, &["commit", "-qm", "baseline"]);
        git(&root, &["init", "--bare", "-q", remote.to_str().unwrap()]);
        git(
            &repo,
            &["remote", "add", "origin", remote.to_str().unwrap()],
        );
        git(&repo, &["push", "-q", "origin", "main"]);
        // Planning approval commonly exists only in the planning-root checkout
        // when Auto starts. Its commit must remain an ancestor of published work.
        std::fs::write(
            repo.join("planning/local-approval.md"),
            "approved locally\n",
        )
        .unwrap();
        git(&repo, &["add", "planning/local-approval.md"]);
        git(&repo, &["commit", "-qm", "approve local plan"]);
        let pi = root.join("pi-fixture");
        std::fs::write(&pi, r#"#!/usr/bin/python3
import json, pathlib, sys, time
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
            project.implementation_states.clear();
            project.chat_slug = format!("auto-e2e-{}", std::process::id());
        }
        app.implement_task(docs[0].path.clone());
        let ctx = egui::Context::default();
        let deadline = Instant::now() + Duration::from_secs(20);
        let mut concurrent_chat = false;
        loop {
            app.tick(0.1, &ctx);
            if !concurrent_chat
                && app
                    .chat_messages()
                    .iter()
                    .any(|m| m.text.starts_with("Manager fixture:"))
            {
                assert!(
                    matches!(&app.screen, Screen::Connected(p) if p.active_implementation.is_some())
                );
                assert!(!app.conversation_busy());
                app.start_turn("Can we discuss planning while the task runs?");
                assert!(app.conversation_busy());
                concurrent_chat = true;
            }
            let finished = matches!(&app.screen, Screen::Connected(project) if !project.queue.running && project.active_implementation.is_none());
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
            assert!(
                project.queue.last_error.is_empty(),
                "{}",
                project.queue.last_error
            );
            for doc in &docs {
                assert_eq!(
                    project.implementation_states.get(&doc.path).unwrap().status,
                    "Done"
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
        assert_eq!(git(&remote, &["rev-list", "--count", "main"]), "5");
        assert_eq!(
            git(&remote, &["show", "main:planning/local-approval.md"]),
            "approved locally"
        );
        assert_eq!(git(&remote, &["show", "main:001-task.txt"]), "implemented");
        assert_eq!(git(&remote, &["show", "main:002-task.txt"]), "implemented");
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
        let planner = root.join(".planner");
        std::fs::create_dir_all(&planner).unwrap();
        std::fs::write(
            planner.join("config.md"),
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
            active_implementation: None,
            active_implementation_ticket: None,
            pr_refresh: None,
            reconciliation: None,
            reconciliation_attempted: Default::default(),
            reconciliation_error: None,
            investigation: None,
            investigation_attempted: Default::default(),
            last_pr_refresh: None,
            implementation_states: Default::default(),
            active_turn: None,
            live_progress: Default::default(),
            next_question_id: None,
            git: Default::default(),
            task_documents: Vec::new(),
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
            log.contains("settings: update stakeholders and identity"),
            "checkpoint subject: {log}"
        );
        let _ = std::fs::remove_dir_all(&root);
    }
}
