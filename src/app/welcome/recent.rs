use crate::core::managed_workspace::{LocalWorkspaceSummary, WorkspaceManager};

/// Stable IDs drive actions; personal dates and readable names drive display.
pub fn paint_recent(
    ui: &mut egui::Ui,
    workspaces: &[LocalWorkspaceSummary],
    selected: &mut Option<uuid::Uuid>,
    busy: bool,
) -> bool {
    let dates = WorkspaceManager::from_state_root()
        .last_opened_dates()
        .unwrap_or_default();
    let mut open = false;
    for workspace in workspaces {
        let date = dates.get(&workspace.id).map_or_else(
            || "Not opened here yet".into(),
            |timestamp| format!("Last opened {}", timestamp.format("%b %d, %Y")),
        );
        let title = if workspace.title.trim().is_empty() {
            "Untitled workspace"
        } else {
            &workspace.title
        };
        ui.push_id(workspace.id, |ui| {
            let response = ui.add_enabled(
                !busy,
                egui::Button::new(format!("{title}\nOn this computer · {date}"))
                    .min_size(egui::vec2(ui.available_width().min(480.0), 56.0))
                    .selected(*selected == Some(workspace.id)),
            );
            if response.clicked() {
                *selected = Some(workspace.id);
                open = true;
            }
        });
    }
    open
}
