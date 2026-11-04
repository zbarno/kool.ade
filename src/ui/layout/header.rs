use super::*;

mod menu;

/// Header content participates in layout, so project names never cover controls.
pub(super) fn paint(
    ui: &mut egui::Ui,
    s: &mut dyn Surface,
    compact: bool,
    settings_open: &mut bool,
) {
    Panel::top("koolade_header")
        .exact_size(if compact { 126.0 } else { 140.0 })
        .frame(
            Frame::NONE
                .fill(theme::PANEL)
                .inner_margin(egui::Margin::symmetric(if compact { 12 } else { 22 }, 10)),
        )
        .show(ui, |ui| {
            let bounds = ui.max_rect();
            ui.painter().line_segment(
                [
                    bounds.left_top() - egui::vec2(22.0, 10.0),
                    bounds.right_top() + egui::vec2(22.0, -10.0),
                ],
                egui::Stroke::new(3.0, theme::PUNCH),
            );
            ui.horizontal(|ui| {
                let texture = brand::logo(ui.ctx(), compact);
                let logo_width = if compact { 94.0 } else { 128.0 };
                ui.add(egui::Image::new(&texture).fit_to_exact_size(egui::vec2(
                    logo_width,
                    logo_width * texture.size_vec2().y / texture.size_vec2().x,
                )));
                if !compact {
                    ui.add_space(14.0);
                    ui.separator();
                    ui.add_space(10.0);
                    let width = (ui.available_width() - 204.0).max(80.0);
                    ui.allocate_ui_with_layout(
                        egui::vec2(width, 40.0),
                        Layout::top_down(egui::Align::Min),
                        |ui| {
                            project_title(ui, s, 21.0);
                            ui.label(theme::helper_text("Your ideas. A clear path to done."));
                        },
                    );
                }
                ui.with_layout(Layout::right_to_left(egui::Align::Center), |ui| {
                    ui.menu_button("Workspace", |ui| menu::paint(ui, s, settings_open));
                    if !compact && ui.button("Settings").clicked() {
                        *settings_open = true;
                    }
                });
            });
            if compact {
                project_title(ui, s, 17.0);
            }
            ui.horizontal(|ui| {
                let busy = s.active_task_count() > 0 || s.is_busy();
                let label = if s.active_task_count() > 0 {
                    format!(
                        "{} {} running",
                        s.active_task_count(),
                        if s.active_task_count() == 1 {
                            "task"
                        } else {
                            "tasks"
                        }
                    )
                } else if busy {
                    "Working".into()
                } else {
                    "Ready".into()
                };
                theme::badge(
                    ui,
                    &label,
                    if busy {
                        theme::ACCENT_SOFT
                    } else {
                        theme::PANEL_ALT
                    },
                    if busy {
                        theme::BLUE_BRIGHT
                    } else {
                        theme::TEXT_DIM
                    },
                )
                .on_hover_text(s.queue_status());
                if !compact {
                    ui.label(theme::metadata_text("PROJECT"));
                }
                let branch = s.git_branch();
                let branch = if branch.is_empty() {
                    "No branch"
                } else {
                    branch
                };
                ui.add_sized(
                    [if compact { 100.0 } else { 240.0 }, 22.0],
                    egui::Label::new(theme::helper_text(branch)).truncate(),
                )
                .on_hover_text(branch);
                ui.with_layout(Layout::right_to_left(egui::Align::Center), |ui| {
                    let (state, color) = if !s.is_git_repo() {
                        ("No repository", theme::TEXT_MUTED)
                    } else if s.git_dirty() {
                        ("Unsaved changes", theme::WARNING)
                    } else {
                        ("Saved to Git", theme::SUCCESS)
                    };
                    ui.add(
                        egui::Label::new(RichText::new(state).size(12.0).color(color)).truncate(),
                    );
                });
            });
            ui.horizontal(|ui| {
                ui.label(theme::metadata_text("Live activity"))
                    .on_hover_text("Observed updates per 10 seconds · last 10 minutes");
                crate::ui::task_activity::header_graph(
                    ui,
                    &s.activity_samples(None),
                    theme::BLUE,
                    16.0,
                );
            });
        });
}

fn project_title(ui: &mut egui::Ui, s: &dyn Surface, size: f32) {
    let title = s.session_title();
    let title = if title.is_empty() {
        "No project connected"
    } else {
        title
    };
    ui.add(
        egui::Label::new(RichText::new(title).size(size).strong().color(theme::TEXT)).truncate(),
    )
    .on_hover_text(title);
}
