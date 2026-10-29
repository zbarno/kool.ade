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
    /// Wizard "start a new project" track (per-launch state only):
    /// project name, editable planning-repository name (suggests
    /// `<name>.packet` when empty), chosen parent folder, and the last
    /// failure surfaced verbatim.
    wiz_project_name: String,
    wiz_packet_name: String,
    wiz_parent_path: String,
    wiz_error: Option<String>,
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
    /// Parent folder picker for the "new project" wizard track: chooses the
    /// directory that will hold the new sibling code + planning pair.
    WizardParent,
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
