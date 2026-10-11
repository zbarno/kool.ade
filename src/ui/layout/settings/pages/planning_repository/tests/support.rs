use super::super::*;
use crate::ui::{
    ApplicationCommand, PlanningCodeRepositoryView, PlanningRepositoryCommand,
    PlanningRepositoryViewModel, Surface, ToastQueue,
};
use crate::{
    core::planning_sync::{
        PlanningConflictChoice, PlanningSyncConflict, PlanningSyncMode, PlanningSyncState,
        PlanningSyncStatus,
    },
    domain::chatlog::ChatMessage,
};
use std::path::PathBuf;

pub(super) struct Probe {
    pub(super) view: Option<PlanningRepositoryViewModel>,
    toast: ToastQueue,
    pub(super) sync_requested: bool,
    pub(super) migration_preview_requested: std::cell::Cell<bool>,
    migration_error: Option<String>,
    pub(super) resolution: Option<PlanningConflictChoice>,
}

impl Default for Probe {
    fn default() -> Self {
        Self {
            view: None,
            toast: ToastQueue::default(),
            sync_requested: false,
            migration_preview_requested: std::cell::Cell::new(false),
            migration_error: None,
            resolution: None,
        }
    }
}

impl Probe {
    pub(super) fn with_state(state: PlanningSyncState) -> Self {
        Self {
            view: Some(PlanningRepositoryViewModel {
                workspace_id: "workspace-123".into(),
                local_path: PathBuf::from("/tmp/planning-workspace"),
                shared: true,
                canonical_ref: "refs/heads/main".into(),
                remote_url: Some("https://github.example/acme/planning.git".into()),
                repositories: vec![PlanningCodeRepositoryView {
                    id: "application".into(),
                    remote: "https://github.example/acme/application.git".into(),
                    checkout: None,
                }],
                sync: status(state),
                sync_running: false,
                migration_available: false,
            }),
            ..Self::default()
        }
    }
}

impl Surface for Probe {
    fn session_title(&self) -> &str {
        "planning repository test"
    }
    fn is_git_repo(&self) -> bool {
        false
    }
    fn git_branch(&self) -> &str {
        ""
    }
    fn git_head(&self) -> &str {
        ""
    }
    fn git_dirty(&self) -> bool {
        false
    }
    fn chat_messages(&self) -> &[ChatMessage] {
        &[]
    }
    fn chat_draft(&mut self) -> &mut String {
        unreachable!()
    }
    fn is_busy(&self) -> bool {
        false
    }
    fn conversation_busy(&self) -> bool {
        false
    }
    fn task_progress(&self, _ticket: &str) -> Option<&crate::harness::LiveProgress> {
        None
    }
    fn task_offer(&self) -> Option<&crate::core::workflow::InterviewBrief> {
        None
    }
    fn implementation_state(
        &self,
        _ticket: &str,
    ) -> Option<&crate::core::implementation::Implementation> {
        None
    }
    fn task_detail_view(&mut self, _ticket: &str) -> Option<crate::ui::task_detail::ViewModel> {
        None
    }
    fn implementation_active(&self, _ticket: &str) -> bool {
        false
    }
    fn implementation_elapsed(&self, _ticket: &str) -> Option<String> {
        None
    }
    fn auto_plan(&self) -> bool {
        false
    }
    fn auto_build(&self) -> bool {
        false
    }
    fn auto_implement(&self) -> bool {
        false
    }
    fn auto_publish(&self) -> bool {
        false
    }
    fn require_independent_checks(&self) -> bool {
        false
    }
    fn queue_status(&self) -> &str {
        ""
    }
    fn live_progress(&self) -> Option<&crate::harness::LiveProgress> {
        None
    }
    fn next_question_id(&self) -> Option<&str> {
        None
    }
    fn spec_text(&self) -> &str {
        ""
    }
    fn toasts(&mut self) -> &mut ToastQueue {
        &mut self.toast
    }
    fn planning_repository(&self) -> Option<PlanningRepositoryViewModel> {
        self.view.clone()
    }
    fn migration_preview(
        &self,
    ) -> Result<crate::core::managed_workspace::MigrationPreview, String> {
        self.migration_preview_requested.set(true);
        Err(self
            .migration_error
            .clone()
            .unwrap_or_else(|| "fixture failure".into()))
    }
    fn dispatch(&mut self, command: ApplicationCommand) {
        let ApplicationCommand::PlanningRepository(command) = command else {
            return;
        };
        match command {
            PlanningRepositoryCommand::SyncNow => self.sync_requested = true,
            PlanningRepositoryCommand::ResolveConflict { choice, .. } => {
                self.resolution = Some(choice)
            }
            _ => {}
        }
    }
}

pub(super) fn status(state: PlanningSyncState) -> PlanningSyncStatus {
    PlanningSyncStatus {
        mode: PlanningSyncMode::Shared,
        state,
        canonical_ref: Some("refs/heads/main".into()),
        confirmed_commit: Some("abcdef123456".into()),
        confirmed_at_ms: Some(1_760_000_000_000),
        pending_operations: 0,
        pending: Vec::new(),
        conflicts: Vec::new(),
        detail: None,
    }
}

pub(super) fn render(
    ctx: &egui::Context,
    probe: &mut Probe,
    events: Vec<egui::Event>,
) -> (String, Vec<(String, egui::Pos2)>) {
    let mut output = ctx.run_ui(
        egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(
                egui::Pos2::ZERO,
                egui::vec2(1100.0, 1200.0),
            )),
            events,
            ..Default::default()
        },
        |ui| {
            egui::CentralPanel::default().show(ui, |ui| {
                paint(ui, probe);
            });
            probe.toasts().show(ui.ctx());
        },
    );
    let labels = output
        .shapes
        .iter()
        .filter_map(|clipped| match &clipped.shape {
            egui::Shape::Text(text) => Some((
                text.galley.text().to_owned(),
                text.visual_bounding_rect().center(),
            )),
            _ => None,
        })
        .collect::<Vec<_>>();
    let text = labels
        .iter()
        .map(|(label, _)| label.as_str())
        .collect::<Vec<_>>()
        .join("\n");
    output.textures_delta.clear();
    (text, labels)
}

pub(super) fn button_center(ctx: &egui::Context, probe: &mut Probe, label: &str) -> egui::Pos2 {
    render(ctx, probe, Vec::new())
        .1
        .into_iter()
        .find_map(|(text, pos)| (text == label).then_some(pos))
        .expect("button text was rendered")
}

pub(super) fn click(ctx: &egui::Context, probe: &mut Probe, pos: egui::Pos2) {
    for pressed in [true, false] {
        let _ = render(
            ctx,
            probe,
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
}

pub(super) fn conflict() -> PlanningSyncConflict {
    PlanningSyncConflict {
        operation_id: uuid::Uuid::from_u128(7),
        path: "state/work/task-7.json".into(),
        record_id: "task-7".into(),
        expected_revision: Some(2),
        base_commit: "abcdef123456".into(),
        local_commit: Some("fedcba987654".into()),
        remote_commit: "123456abcdef".into(),
        fields: vec!["title".into()],
        local_value: Some("local title".into()),
        remote_value: Some("remote title".into()),
    }
}
