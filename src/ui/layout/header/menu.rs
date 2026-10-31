use super::*;

pub(super) fn paint(ui: &mut egui::Ui, s: &mut dyn Surface, settings_open: &mut bool) {
    ui.set_min_width(230.0);
    ui.label(theme::metadata_text("WORKSPACE"));
    workspace_repositories::paint_menu(ui, s);
    action(ui, s, "Open workspace…", HeaderAction::OpenWorkspace);
    action(ui, s, "Refresh repository", HeaderAction::Refresh);
    ui.separator();
    ui.label(theme::metadata_text("PROJECT RESOURCES"));
    action(ui, s, "Import references…", HeaderAction::Import);
    action(ui, s, "MCP servers…", HeaderAction::McpServers);
    ui.separator();
    if ui.button("Settings…").clicked() {
        *settings_open = true;
        ui.close();
    }
    ui.label(theme::helper_text("Appearance, tools, automation & people"));
    ui.separator();
    action(ui, s, "Disconnect", HeaderAction::Disconnect);
}

fn action(ui: &mut egui::Ui, s: &mut dyn Surface, label: &str, action: HeaderAction) {
    if ui.button(label).clicked() {
        s.dispatch(ApplicationCommand::HeaderAction(action));
        ui.close();
    }
}
