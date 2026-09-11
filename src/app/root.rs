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

/// Root of the packet app.
pub struct PacketApp {
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

/// Native window options for [`eframe::run_native`].
pub fn options() -> eframe::NativeOptions {
    let mut vp = egui::ViewportBuilder::default();
    vp = vp
        .with_inner_size([1280.0, 820.0])
        .with_min_inner_size([1000.0, 640.0]);
    eframe::NativeOptions {
        viewport: vp,
        ..Default::default()
    }
}

impl Default for PacketApp {
    fn default() -> Self {
        let toasts = ToastQueue::default();

        Self {
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
                            project.live_progress.update(p)
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
                project.active_implementation_ticket = None;
                project.live_progress = Default::default();
                project.refresh_implementations();
                project.last_pr_refresh = None;
                let text = match result {
                    Ok(record) => format!(
                        "Implementation verified. Pull request: {}",
                        record.pr_url.unwrap_or_default()
                    ),
                    Err(error) => format!(
                        "Implementation stopped: {error}\nExisting work is preserved; use Resume implementation to continue."
                    ),
                };
                project.remember_chat(vec![ChatMessage::new(ChatRole::Agent, text, None)]);
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
        let period = match &self.screen {
            Screen::Connected(p)
                if (p.active_turn.is_some() || p.active_implementation.is_some()) =>
            {
                Duration::from_millis(120)
            }
            _ => Duration::from_millis(800),
        };
        ctx.request_repaint_after(period);
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
            crate::core::ownership::synthesize_missing_owners(
                project.state.items.as_slice(),
                &project.state.config.stakeholders,
            ),
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
                project.next_question_id = normalized.next_question_id.clone();
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
                project.remember_chat(chat);
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
                project.remember_chat(chat);
                self.toasts.danger(format!(
                    "Rejected: {}",
                    problems.first().map(String::as_str).unwrap_or("")
                ));
            }
            TurnOutcome::HarnessFailed { error, .. } => {
                project.remember_chat(vec![ChatMessage::new(
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
                if p.state.workflow.ready(p.state.spec_text.as_deref())
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
        if project.active_turn.is_some() || project.active_implementation.is_some() {
            return;
        }
        let recent = project.recent_chat_tuples(6, 1200);
        project.remember_chat(vec![ChatMessage::new(ChatRole::User, text, None)]);
        let inputs = crate::core::turn::TurnInputs {
            state: project.state.clone(),
            user_message: text.to_string(),
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

    fn is_busy(&self) -> bool {
        matches!(&self.screen, Screen::Connected(p) if p.active_turn.is_some() || p.active_implementation.is_some())
    }

    fn live_progress(&self) -> Option<&crate::harness::LiveProgress> {
        match &self.screen {
            Screen::Connected(p)
                if p.active_turn.is_some() || p.active_implementation.is_some() =>
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
                    && p.state.workflow.ready(p.state.spec_text.as_deref()) =>
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
    fn implement_task(&mut self, ticket: String) {
        if self.is_busy() {
            return;
        }
        if let Screen::Connected(p) = &mut self.screen {
            if p.implementation_states
                .get(&ticket)
                .is_some_and(|state| state.pr_url.is_some())
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
            p.remember_chat(vec![ChatMessage::new(ChatRole::User, format!("Implement ticket {ticket}. Resume its worktree if it exists, verify the result, and create a pull request."), None)]);
            p.live_progress = crate::harness::LiveProgress {
                activity: Some("Starting implementation…".into()),
                ..Default::default()
            };
            p.active_implementation_ticket = Some(ticket.clone());
            p.active_implementation = Some(crate::core::implementation::Controller::start(
                p.state.repo_root.clone(),
                ticket,
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
                if let Some(ctrl) = &p.active_implementation {
                    ctrl.request_cancel();
                }
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
                            self.toasts.success(format!("MCP servers saved · checkpoint {sha}"));
                        }
                        Some(parse_error) => {
                            d.keep_open = true;
                            d.warning = Some(format!(
                                "Not valid JSON: {parse_error} — saved anyway; \
                                 consumers sit outside the planner. Turns \
                                 advertise it verbatim until fixed."
                            ));
                            d.feedback =
                                Some((true, format!("Saved · checkpoint {sha} — kept open, see warning")));
                        }
                    },
                    crate::artifacts::mcp_io::McpSaveOp::Clear => {
                        d.keep_open = false;
                        d.feedback = Some((true, format!("Cleared · checkpoint {sha}")));
                        self.toasts.success(format!("MCP servers cleared · checkpoint {sha}"));
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

    fn fixture() -> PacketApp {
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
                branch: "packet/fixture".into(),
                base: "main".into(),
                base_commit: "fixture".into(),
                worktree: root.join("worktree"),
                status: if i == 1 { "PR created" } else { "Done" }.into(),
                detail: String::new(),
                pr_url: Some(format!("https://github.com/fixture/repo/pull/{i}")),
                verified_head: Some("fixture".into()),
                pr_state: Some(pr_state.into()),
                pr_checked_at: None,
                pr_check_attempted_at: None,
                pr_check_error: None,
            };
            states.insert(record.ticket.clone(), record);
        }
        PacketApp {
            screen: Screen::Connected(Project {
                state: crate::core::state::PlannerState::load(&root).unwrap(),
                chat_slug: "unused".into(),
                chat: Vec::new(),
                draft: String::new(),
                active_implementation: None,
                active_implementation_ticket: None,
                implementation_states: states,
                pr_refresh: None,
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

    fn frame(
        app: &mut PacketApp,
        ctx: &egui::Context,
        events: Vec<egui::Event>,
    ) -> egui::FullOutput {
        let mut output = ctx.run_ui(
            egui::RawInput {
                screen_rect: Some(egui::Rect::from_min_size(
                    egui::Pos2::ZERO,
                    egui::vec2(1800.0, 900.0),
                )),
                events,
                ..Default::default()
            },
            |ui| crate::ui::layout::paint(ui, app),
        );
        output.textures_delta.clear();
        output
    }
    fn text_position(output: &egui::FullOutput, needle: &str) -> Option<egui::Pos2> {
        output.shapes.iter().find_map(|shape| {
            if let egui::Shape::Text(text) = &shape.shape {
                if text.galley.text() == needle {
                    return Some(text.pos + text.galley.size() * 0.5);
                }
            }
            None
        })
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
        app.implement_task("planning/tasks/fixture/002-task.md".into());
        assert!(
            !app.is_busy(),
            "published tasks must not start another agent"
        );
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
        let root =
            std::env::temp_dir().join(format!("packet_caches_mira_{}", std::process::id()));
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
            state,
            chat_slug: "test-slug".into(),
            chat: Vec::new(),
            draft: String::new(),
            active_implementation: None,
            active_implementation_ticket: None,
            pr_refresh: None,
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
