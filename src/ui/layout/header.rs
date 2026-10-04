use super::*;

pub(super) fn paint(
    ui: &mut egui::Ui,
    s: &mut dyn Surface,
    compact: bool,
    settings_open: &mut bool,
) {
    let mut open = *settings_open;
    Panel::top("koolade_header")
        .exact_size(if compact { 72.0 } else { 96.0 })
        .frame(
            Frame::NONE
                .fill(theme::PUNCH_DEEP)
                .inner_margin(egui::Margin::symmetric(if compact { 12 } else { 22 }, 2)),
        )
        .show(ui, |ui| {
            let bounds = ui
                .max_rect()
                .expand2(egui::vec2(if compact { 12.0 } else { 22.0 }, 2.0));
            let texture = brand::splash(ui.ctx());
            let splash_width = (bounds.width() * 0.82).min(980.0);
            let splash_height = splash_width * texture.size_vec2().y / texture.size_vec2().x;
            let splash = egui::Rect::from_min_size(
                bounds.left_top() + egui::vec2(-16.0, -splash_height * 0.35),
                egui::vec2(splash_width, splash_height),
            );
            ui.painter().with_clip_rect(bounds).image(
                texture.id(),
                splash,
                egui::Rect::from_min_max(egui::Pos2::ZERO, egui::pos2(1.0, 1.0)),
                egui::Color32::from_white_alpha(225),
            );
            let activity_height = if compact { 28.0 } else { 18.0 };
            let activity_strip = egui::Rect::from_min_max(
                egui::pos2(bounds.left(), bounds.bottom() - activity_height),
                bounds.right_bottom(),
            );
            ui.painter()
                .rect_filled(activity_strip, 0, egui::Color32::from_rgb(23, 10, 17));
            ui.painter().line_segment(
                [bounds.left_bottom(), bounds.right_bottom()],
                egui::Stroke::new(2.0, theme::PUNCH_BRIGHT),
            );
            ui.horizontal(|ui| {
                ui.set_min_height(if compact { 40.0 } else { 64.0 });
                let texture = brand::logo(ui.ctx(), compact);
                let logo_width = if compact { 120.0 } else { 128.0 };
                let logo_size = egui::vec2(
                    logo_width,
                    logo_width * texture.size_vec2().y / texture.size_vec2().x,
                );
                ui.add(egui::Image::new(&texture).fit_to_exact_size(logo_size));
                ui.add_space(12.0);
                ui.with_layout(Layout::right_to_left(egui::Align::Center), |ui| {
                    ui.menu_button("Workspace", |ui| {
                        if ui.button("Settings…").clicked() {
                            open = true;
                            ui.close();
                        }
                        ui.separator();
                        let mut auto_implement = s.auto_implement();
                        if ui.checkbox(&mut auto_implement, "Auto-Implement").changed() {
                            s.dispatch(ApplicationCommand::SetAutoBuild {
                                enabled: auto_implement,
                            });
                        }
                        ui.separator();
                        workspace_repositories::paint_menu(ui, s);
                        ui.separator();
                        for (label, action) in [
                            ("Import references", HeaderAction::Import),
                            ("Stakeholders & ownership", HeaderAction::Stakeholders),
                            ("MCP servers", HeaderAction::McpServers),
                            ("Refresh repository", HeaderAction::Refresh),
                            ("Open workspace", HeaderAction::OpenWorkspace),
                            ("Disconnect", HeaderAction::Disconnect),
                        ] {
                            if ui.button(label).clicked() {
                                s.dispatch(ApplicationCommand::HeaderAction(action));
                                ui.close();
                            }
                        }
                    });
                    {
                        ui.add_space(8.0);
                        let state = if !s.is_git_repo() {
                            if compact { "No repo" } else { "No repository" }
                        } else if s.git_dirty() {
                            if compact {
                                "Changes"
                            } else {
                                "Uncommitted changes"
                            }
                        } else if compact {
                            "Saved"
                        } else {
                            "Saved to git"
                        };
                        ui.add(
                            egui::Label::new(
                                RichText::new(state).size(12.0).color(theme::TEXT_DIM),
                            )
                            .truncate(),
                        );
                        let (dot, _) =
                            ui.allocate_exact_size(egui::vec2(7.0, 7.0), egui::Sense::hover());
                        ui.painter().circle_filled(
                            dot.center(),
                            3.5,
                            if !s.is_git_repo() {
                                theme::TEXT_MUTED
                            } else if s.git_dirty() {
                                theme::WARNING
                            } else {
                                theme::SUCCESS
                            },
                        );
                        ui.add_space(8.0);
                        let branch = s.git_branch();
                        let expanded_id = egui::Id::new("koolade_branch_badge_expanded");
                        let mut expanded = ui
                            .ctx()
                            .data_mut(|data| data.get_temp::<bool>(expanded_id).unwrap_or(false));
                        let branch_width = if expanded {
                            ui.available_width().clamp(200.0, 560.0)
                        } else if compact {
                            64.0
                        } else {
                            90.0
                        };
                        let mut job = egui::text::LayoutJob::simple(
                            branch.to_owned(),
                            egui::FontId::proportional(12.0),
                            theme::TEXT_DIM,
                            branch_width,
                        );
                        if !expanded {
                            job.wrap.max_rows = 1;
                        }
                        let text = ui.painter().layout_job(job);
                        let (rect, response) = ui.allocate_exact_size(
                            egui::vec2(text.size().x + 24.0, text.size().y + 12.0),
                            egui::Sense::click(),
                        );
                        ui.painter().rect_filled(rect, 7, theme::PANEL_ALT);
                        ui.painter().rect_stroke(
                            rect,
                            7,
                            egui::Stroke::new(1.0, theme::BORDER),
                            egui::StrokeKind::Inside,
                        );
                        ui.painter().galley(
                            rect.center() - text.size() / 2.0,
                            text,
                            theme::TEXT_DIM,
                        );
                        if response.clicked() {
                            expanded = !expanded;
                            ui.ctx()
                                .data_mut(|data| data.insert_temp(expanded_id, expanded));
                        }
                        response.on_hover_text(if expanded {
                            "Click to collapse the branch name"
                        } else {
                            "Click to expand the full branch name"
                        });
                    }
                });
            });
            // Anchor the project title to the panel center so its position does
            // not depend on the widths of the logo and workspace controls.
            let project_width = ui.available_width().min(440.0);
            let project_rect = egui::Rect::from_center_size(
                egui::pos2(
                    bounds.center().x,
                    bounds.top() + if compact { 22.0 } else { 32.0 },
                ),
                egui::vec2(project_width, 48.0),
            );
            egui::Area::new(egui::Id::new("koolade_project_heading"))
                .order(egui::Order::Foreground)
                .fixed_pos(project_rect.min)
                .show(ui.ctx(), |ui| {
                    ui.set_width(project_width);
                    ui.with_layout(Layout::top_down(egui::Align::Center), |ui| {
                        let project = s.session_title();
                        ui.add(
                            egui::Label::new(
                                RichText::new(if project.is_empty() {
                                    "No project connected"
                                } else {
                                    project
                                })
                                .size(if compact { 18.0 } else { 22.0 })
                                .strong()
                                .color(theme::TEXT),
                            )
                            .truncate(),
                        )
                        .on_hover_text(project);
                        let (work_status, status_color) = if s.active_task_count() > 0 {
                            (
                                format!("Working · {} tasks", s.active_task_count()),
                                theme::BLUE_BRIGHT,
                            )
                        } else if s.is_busy() {
                            ("Working".to_owned(), theme::BLUE_BRIGHT)
                        } else {
                            ("Ready".to_owned(), theme::TEXT_DIM)
                        };
                        ui.label(
                            RichText::new(work_status)
                                .size(if compact { 11.0 } else { 12.0 })
                                .color(status_color),
                        )
                        .on_hover_text(s.queue_status());
                    });
                });
            ui.horizontal(|ui| {
                ui.label(
                    RichText::new("Live activity")
                        .size(11.0)
                        .color(theme::TEXT_MUTED),
                )
                .on_hover_text("Updates per 10 seconds · last 10 minutes");
                ui.allocate_ui_with_layout(
                    egui::vec2(
                        ui.available_width().max(40.0),
                        if compact { 18.0 } else { 16.0 },
                    ),
                    Layout::top_down(egui::Align::Min),
                    |ui| {
                        crate::ui::task_activity::header_graph(
                            ui,
                            &s.activity_samples(None),
                            theme::PUNCH,
                            if compact { 18.0 } else { 16.0 },
                        );
                    },
                );
            });
        });
    *settings_open = open;
}
