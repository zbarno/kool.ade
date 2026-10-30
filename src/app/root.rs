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
#[cfg(test)]
pub(crate) mod harness_selection;
#[cfg(not(test))]
mod harness_selection;

mod adoption;
mod agent_updates;
mod attention;
mod bootstrap_conversation;
mod clarification;
mod clipboard;
pub(crate) mod code_review_arbitration;
mod connection;
mod dependency_authorization;
mod desktop_app;
mod dialogs;
mod implementation_controller;
mod implementation_decision;
mod opportunity_actions;
pub(crate) mod plan_review;
pub(crate) mod spec_reconciliation;
pub(crate) use spec_reconciliation::SpecReconciliation;
mod initialization;
pub(crate) mod planner_pass;
mod planning_repository;
mod planning_repository_surface;
mod planning_work;
mod repository_switcher;
mod requested_action;
pub(in crate::app::root) mod runtime_dependencies;
pub(crate) mod runtime_qa_applicability;
mod runtime_qa_scenarios;
mod setup_attention;
mod setup_repair;
mod specification_trace_navigation;
mod surface;
mod task_batch;
#[cfg(test)]
#[path = "root/task_detail_tests.rs"]
mod task_detail_tests;
mod task_target;
#[cfg(test)]
pub(crate) mod test_runtime_qa;
mod tick;
mod ui_actions;
mod welcome_wizard;
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
    conn_create_title: String,
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
    planning_sync_job: Option<
        std::thread::JoinHandle<(
            uuid::Uuid,
            Result<crate::core::planning_sync::PlanningSyncStatus, String>,
        )>,
    >,
    conn_join_remote: String,
    conn_workspace_id: Option<uuid::Uuid>,
    /// Which welcome card the native folder picker should fill: the code
    /// repository card (`conn_path`) by default, or the planning repository
    /// card (`conn_join_remote`) when the join row's Browse… button raised
    /// the picker. Refreshed per welcome frame; see `BrowseTarget`.
    conn_browse_target: BrowseTarget,
    /// Active welcome wizard (per-launch state; a closed draft is gone).
    /// Side effects occur only when the CONFIRM step commits.
    wiz: Option<WelcomeWizard>,
    /// Set by the wizard's CloneRepository commit while the shared clone
    /// worker runs; consumed by [`Self::poll_clone_job`].
    pending_clone_pairing: Option<PendingClonePairing>,
    /// Test-only seam: when `Some`, [`Self::begin_clone`] hands the worker
    /// THIS computation instead of the real `welcome::perform_clone`, so
    /// dispatch can be proven hermetically (no network, no real git
    /// process). Shipping code never writes it (`Default` installs `None`).
    clone_computation_override: Option<CloneWorkerCalc>,
    /// Cached routing identity (rebuilt after connect/adoption/settings).
    cached_user: CurrentUser,
    attention: std::collections::BTreeMap<std::path::PathBuf, attention::Status>,
    setup_attention: Vec<super::setup_attention::SetupIssue>,
    /// Read-only projection of the machine-local setup task store.
    machine_setup_tasks: Vec<crate::persistence::setup_tasks::LocalTask>,
    setup_attention_notified: Option<String>,
    setup_probe: Option<std::thread::JoinHandle<setup_attention::ProbeOutput>>,
    setup_probe_cancel: Option<std::sync::Arc<std::sync::atomic::AtomicBool>>,
    setup_commit_lock: std::sync::Arc<std::sync::Mutex<()>>,
    #[cfg(test)]
    setup_task_state_path: std::path::PathBuf,
    setup_retry_requested: bool,
    setup_refresh_pending: Option<bool>,
    setup_image_preparation: super::bootstrap_image_prepare::Controller,
    claude_install: super::bootstrap_remediation::ClaudeInstallController,
    setup_remediation: super::bootstrap_remediation::Controller,
    bootstrap_conversation: bootstrap_conversation::Controller,
    #[cfg(test)]
    attention_fixture: std::collections::BTreeMap<String, crate::core::attention::Brief>,
    #[cfg(test)]
    _test_temp_roots: Vec<TestTempRoot>,
    /// Synthesized ownership-gap items for the side pane.
    synth: Vec<OpenItem>,
}

#[cfg(test)]
struct TestTempRoot(std::path::PathBuf);

#[cfg(test)]
impl Drop for TestTempRoot {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

pub(crate) fn configured_harness_for_project(
    override_harness: &mut Option<Box<dyn crate::harness::AiHarness>>,
    work_type: Option<&str>,
    project_routes: &std::collections::BTreeMap<
        String,
        crate::persistence::harness_settings::WorkRoute,
    >,
) -> Box<dyn crate::harness::AiHarness> {
    harness_selection::configured_harness_for_project(override_harness, work_type, project_routes)
}

pub(crate) fn role_for_work_category(
    category: &str,
) -> Option<crate::execution::contract::HarnessRole> {
    harness_selection::role_for_work_category(category)
}

/// Actionable task-detail nudge for unsatisfiable CLI routing (see
/// [`crate::app::root::harness_selection`]). Re-exported so the UI layer
/// can raise the nudge without reaching into a private module.
pub(crate) use harness_selection::cli_routing_notice;

pub(crate) fn configured_harness_for_project_task(
    override_harness: &mut Option<Box<dyn crate::harness::AiHarness>>,
    work_type: Option<&str>,
    project_routes: &std::collections::BTreeMap<
        String,
        crate::persistence::harness_settings::WorkRoute,
    >,
    task_routes: &std::collections::BTreeMap<
        String,
        crate::persistence::harness_settings::WorkRoute,
    >,
) -> Box<dyn crate::harness::AiHarness> {
    harness_selection::configured_harness_for_project_task(
        override_harness,
        work_type,
        project_routes,
        task_routes,
    )
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
    planning_store: crate::artifacts::planning_store::PlanningStore,
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
/// Which welcome card a native folder-picker selection should fill. The
/// code-repository card and the planning-repository card share one picker;
/// this records which field owns the incoming selection. Reset to `CodeRepo`
/// at the start of every welcome frame; the join row's Browse… flips it to
/// `PlanningRepo` in the same frame before the picker result lands.
#[derive(Clone, Copy, PartialEq, Eq)]
pub(super) enum BrowseTarget {
    CodeRepo,
    PlanningRepo,
    /// Wizard draft field filled by the native folder picker.
    Wizard(WizField),
}

/// The three first-party journey starters offered on the welcome landing.
#[derive(Clone, Copy, PartialEq, Eq)]
pub(super) enum WelcomeEntry {
    /// The code repository already exists locally; Kool.ad/e only adds the
    /// planning sibling and registers the checkout.
    ExistingCode,
    /// The code repository must be downloaded first; Kool.ad/e clones it
    /// under a chosen destination, then pairs the planning sibling.
    CloneRepository,
    /// Nothing exists yet; both repositories are created from scratch.
    BlankProject,
}

impl WelcomeEntry {
    pub(super) fn title(self) -> &'static str {
        match self {
            Self::ExistingCode => "Set up planning for your code",
            Self::CloneRepository => "Clone a code repository and set up planning",
            Self::BlankProject => "Start a brand-new project",
        }
    }
}

/// Which wizard field the native folder picker should fill.
#[derive(Clone, Copy, PartialEq, Eq)]
pub(super) enum WizField {
    /// ExistingCode entry: the folder that IS the code repository root.
    CodePath,
    /// BlankProject entry: the parent folder for the fresh sibling pair.
    Parent,
}

/// Draft state for ONE active welcome wizard. Closing a draft discards it;
/// nothing touches the disk until the confirm step commits.
pub(super) struct WelcomeWizard {
    entry: WelcomeEntry,
    /// 0 = gather inputs, 1 = confirm the planned side effects.
    step: u8,
    project_name: String,
    /// Empty means "use the suggested <code folder>.packet name".
    packet_name: String,
    /// ExistingCode: the code repository folder itself.
    code_path: String,
    /// CloneRepository: the repository URL to download (cloned into
    /// `~/kool-ade-workspaces/<repo>` by the shared clone engine).
    url: String,
    /// BlankProject: parent folder for the fresh pair.
    parent: String,
    error: Option<String>,
}

impl WelcomeWizard {
    pub(super) fn new(entry: WelcomeEntry) -> Self {
        Self {
            entry,
            step: 0,
            project_name: String::new(),
            packet_name: String::new(),
            code_path: String::new(),
            url: String::new(),
            parent: String::new(),
            error: None,
        }
    }

    /// Absolute destination the clone engine will use for CloneRepository.
    pub(super) fn clone_destination(&self) -> Option<std::path::PathBuf> {
        let home = std::env::var_os("HOME")?;
        let name = self.code_folder_name();
        if name.is_empty() {
            return None;
        }
        Some(crate::app::welcome::github::clone_destination(
            &name,
            &std::path::PathBuf::from(home),
        ))
    }

    /// Effective project name: the typed title, else derived from the code
    /// source (folder basename for existing code, repository segment for a
    /// URL). Titles are cosmetic; the derived value only fills the field.
    pub(super) fn effective_project_name(&self) -> String {
        let trimmed = self.project_name.trim();
        if !trimmed.is_empty() {
            return trimmed.to_string();
        }
        let basename = match self.entry {
            WelcomeEntry::ExistingCode => self.code_path.trim(),
            WelcomeEntry::CloneRepository => self.url.trim(),
            WelcomeEntry::BlankProject => "", // blank projects need an authored title
        };
        std::path::Path::new(basename)
            .file_stem()
            .and_then(|name| name.to_str())
            .unwrap_or("")
            .to_owned()
    }

    /// Code folder name where the code repository WILL LIVE after the commit:
    /// the chosen folder as-is (existing), `<dest>/<repo>` (clone), or the
    /// slugged title (blank).
    pub(super) fn code_folder_name(&self) -> String {
        match self.entry {
            WelcomeEntry::ExistingCode => std::path::Path::new(self.code_path.trim())
                .file_name()
                .and_then(|name| name.to_str())
                .unwrap_or("")
                .to_owned(),
            WelcomeEntry::CloneRepository => {
                let raw = self.url.trim();
                std::path::Path::new(raw)
                    .file_stem()
                    .and_then(|name| name.to_str())
                    .unwrap_or("")
                    .to_owned()
            }
            WelcomeEntry::BlankProject => {
                crate::app::root::KooladeApp::project_title_to_folder_name(
                    self.effective_project_name().as_str(),
                )
            }
        }
    }

    /// Planning repository name actually used on the confirm step.
    pub(super) fn effective_packet_name(&self) -> String {
        let typed = self.packet_name.trim();
        if !typed.is_empty() {
            return typed.to_string();
        }
        format!("{}.packet", self.code_folder_name())
    }

    /// Human-readable bullet list of EXACTLY what the commit writes to disk;
    /// rendered on the confirm step so the operator consents to the real plan.
    pub(super) fn disk_plan_lines(&self) -> Vec<String> {
        let packet = self.effective_packet_name();
        match self.entry {
            WelcomeEntry::ExistingCode => {
                let code = self.code_path.trim();
                vec![
                    format!(
                        "Leaves the code repository at “{code}” completely untouched (history, files, remotes)."
                    ),
                    format!("Creates the planning repository “{}” beside it.", packet),
                    "Registers the code folder as a code repository of that planning repository."
                        .to_string(),
                    "Opens the planning board for this project.".to_string(),
                ]
            }
            WelcomeEntry::CloneRepository => {
                let name = self.code_folder_name();
                let shown = match self.clone_destination() {
                    Some(dest) => dest.display().to_string(),
                    None => format!("$HOME/kool-ade-workspaces/{name}"),
                };
                vec![
                    format!(
                        "Downloads the repository into “{shown}” (that folder must not exist yet)."
                    ),
                    format!("Creates the planning repository “{packet}” beside the clone."),
                    "Registers the clone as a code repository of that planning repository."
                        .to_string(),
                    "Opens the planning board for this project.".to_string(),
                ]
            }
            WelcomeEntry::BlankProject => {
                let name = self.code_folder_name();
                let parent = self.parent.trim();
                vec![
                    format!(
                        "Creates a fresh code repository “{parent}/{name}” (one placeholder README commit)."
                    ),
                    format!(
                        "Creates the planning repository “{parent}/{packet}” beside it.",
                        packet = self.effective_packet_name()
                    ),
                    "Registers the code repository in the planning repository.".to_string(),
                    "Opens the planning board for this project.".to_string(),
                ]
            }
        }
    }
}

/// The pending clone→pair intent handed to the clone worker's completion hook.
/// Consumed on BOTH clone outcomes; discarded if the wizard is abandoned.
pub(super) struct PendingClonePairing {
    /// Cosmetic title for the planning workspace; may be empty (derived).
    pub(super) project_name: String,
    /// Empty means the suggested `<repo>.packet`.
    pub(super) packet_name: String,
}

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
    AddCodeRepository(app_dialogs::DlgCodeRepository),
    Settings(DlgSettings),
    ProjectSettings(app_dialogs::DlgProjectSettings),
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

// ------------------------------------------------------------------- Surface
#[cfg(test)]
#[path = "root/board_tests.rs"]
pub(crate) mod board_tests;
#[cfg(test)]
#[path = "root/icon_tests.rs"]
mod icon_tests;
#[cfg(test)]
#[path = "root/self_improvement_test_support.rs"]
pub(crate) mod self_improvement_test_support;
#[cfg(test)]
#[path = "root/workspace_tests.rs"]
mod tests;

const NO_SPEC_PLACEHOLDER: &str = "# No specification yet

Describe what you are building in the chat. The planner will draft this page for you and keep every subsequent revision under git.";
