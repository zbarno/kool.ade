//! `KooladeApp`: eframe root. Owns the connect screen and the connected
//! screen; drains the turn bus; delegates all pixels to `crate::ui`.

use std::time::{Duration, Instant};

#[cfg(test)]
use crate::app::dialogs::DlgBrowse;
use crate::app::dialogs::{self as app_dialogs, DlgImport, DlgMcp, DlgSettings};
use crate::app::session::{self, Project};
use crate::app::welcome;
use crate::core::implementation::{ImplementationStatus, PullRequestState};
use crate::core::turn::{TurnController, TurnEvt, TurnOutcome};
use crate::domain::chatlog::{ChatMessage, ChatRole};
use crate::domain::item::OpenItem;
use crate::domain::user::CurrentUser;
use crate::ui::{Surface, ToastQueue};

#[cfg(test)]
#[path = "conversation_tests.rs"]
mod conversation_tests;

#[path = "feature_approval.rs"]
mod feature_approval;
mod harness_selection;

mod adoption;
mod agent_updates;
mod attention;
mod connection;
mod desktop_app;
mod dialogs;
mod implementation_controller;
mod implementation_decision;
mod planning_work;
mod repository_switcher;
mod requested_action;
mod setup_attention;
mod surface;
mod task_batch;
#[cfg(test)]
#[path = "root/task_detail_tests.rs"]
mod task_detail_tests;
mod tick;
mod ui_actions;
mod workspace;
use task_batch::has_current_task_batch;

/// Root of the koolade app.
pub struct KooladeApp {
    task_harness: Option<Box<dyn crate::harness::AiHarness>>,
    pending_feature_generation: Option<(std::path::PathBuf, String, String, String)>,
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
    /// Per-process metering identity stamped on time-ledger intervals so a
    /// crashed-but-unflushed span is identifiable on inspection (AD-4).
    session_id: String,
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
    setup_attention: Option<super::setup_attention::SetupIssue>,
    setup_attention_notified: Option<String>,
    setup_probe: Option<std::thread::JoinHandle<Option<super::setup_attention::SetupIssue>>>,
    setup_retry_requested: bool,
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
    harness_selection::configured_harness(override_harness)
}

pub(super) fn configured_harness_for(
    override_harness: &mut Option<Box<dyn crate::harness::AiHarness>>,
    work_type: Option<&str>,
) -> Box<dyn crate::harness::AiHarness> {
    harness_selection::configured_harness_for(override_harness, work_type)
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
/// to the single connect authority ([`KooladeApp::submit_connect`]).
struct CloneJob {
    /// Badge text for the card's status line: "github.com/{owner}/{repo}".
    url_display: String,
    /// Repository segment as pasted (drives the "Cloning {repo} …" line).
    repo: String,
    join: std::thread::JoinHandle<Result<std::path::PathBuf, crate::error::AppError>>,
}

/// Signature of the stand-in computation for the clone worker (see
/// [`KooladeApp::clone_computation_override`]): canonical url + repo
/// segment -> the destination placed on success. Behind an Arc so a test
/// fixture can be shared with the spawned thread.
type CloneWorkerCalc = std::sync::Arc<
    dyn Fn(String, String) -> Result<std::path::PathBuf, crate::error::AppError> + Send + Sync,
>;

enum Dialog {
    Import(DlgImport),
    Settings(DlgSettings),
    HarnessSetup(app_dialogs::DlgHarnessSetup),
    Mcp(DlgMcp),
    #[cfg(test)]
    Browse(DlgBrowse),
}

/// Native window options for [`eframe::run_native`]. The minimum stays below
/// the compact-layout breakpoint so the stacked workspace is reachable on
/// smaller displays.
pub fn options() -> eframe::NativeOptions {
    let vp = egui::ViewportBuilder::default()
        .with_inner_size([1480.0, 900.0])
        .with_min_inner_size([360.0, 480.0])
        .with_icon(application_icon());
    eframe::NativeOptions {
        viewport: vp,
        ..Default::default()
    }
}

fn application_icon() -> egui::IconData {
    const ICON_SIZE: u32 = 128;

    let icon = image::load_from_memory(include_bytes!("../../assets/brand/app-icon.png"))
        .expect("embedded Kool.ad/e app icon must be a valid image")
        .into_rgba8();
    let icon = image::imageops::resize(
        &icon,
        ICON_SIZE,
        ICON_SIZE,
        image::imageops::FilterType::Lanczos3,
    );

    egui::IconData {
        rgba: icon.into_raw(),
        width: ICON_SIZE,
        height: ICON_SIZE,
    }
}

impl Default for KooladeApp {
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
            session_id: uuid::Uuid::new_v4().hyphenated().to_string(),
            last_git_refresh: Instant::now(),
            display_refresh: None,
            clone_job: None,
            clone_computation_override: None,
            cached_user: CurrentUser::new("", Vec::new()),
            attention: Default::default(),
            setup_attention: None,
            setup_attention_notified: None,
            setup_probe: None,
            setup_retry_requested: false,
            #[cfg(test)]
            attention_fixture: Default::default(),
            synth: Vec::new(),
        }
    }
}

// ------------------------------------------------------------------------ tick

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
impl KooladeApp {
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

#[cfg(test)]
#[path = "root/board_tests.rs"]
mod board_tests;
#[cfg(test)]
#[path = "root/icon_tests.rs"]
mod icon_tests;
#[cfg(test)]
#[path = "root/workspace_tests.rs"]
mod tests;

const NO_SPEC_PLACEHOLDER: &str = "# No specification yet

Describe what you are building in the chat. The planner will draft this page for you and keep every subsequent revision under git.";
