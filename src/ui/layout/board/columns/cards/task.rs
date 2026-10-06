use super::super::super::super::task_cards;
use super::super::super::super::*;

pub(in crate::ui::layout::board::columns) fn task(
    ui: &mut egui::Ui,
    s: &mut dyn Surface,
    board: &crate::ui::planning_board::ViewModel,
    doc: &crate::artifacts::task_docs::TaskDocument,
    column: usize,
    selected_path: &mut Option<String>,
) {
    let active = s.implementation_active(&doc.path);
    let activity_active = s.activity_active(&doc.path);
    let complete = task_board_column(s, &doc.path) == 4;
    task_cards::board_card(
        ui,
        &doc.path,
        None,
        activity_active,
        column == 3,
        complete,
        |ui| {
            let blocked_by = matches!(column, 0 | 3)
                .then(|| dependency_blocker(s, board, doc))
                .flatten();
            if let Some(blocked_by) = blocked_by {
                super::super::attention::badge(ui, super::super::attention::Kind::Blocked);
                ui.label(
                    RichText::new(format!("Waiting for {blocked_by}"))
                        .size(12.0)
                        .color(theme::TEXT_DIM),
                );
            } else if column == 3 {
                super::super::attention::badge(
                    ui,
                    super::super::attention::task_kind(s, &doc.path),
                );
            }
            super::super::super::presentation::metadata(ui, None, &task_key(&doc.path));
            if column == 3
                && ui
                    .add_sized(
                        [ui.available_width(), 32.0],
                        egui::Button::new(
                            RichText::new("Review next action")
                                .size(12.0)
                                .strong()
                                .color(theme::WARNING),
                        )
                        .fill(egui::Color32::from_rgb(52, 42, 20))
                        .stroke(egui::Stroke::new(1.0, egui::Color32::from_rgb(102, 77, 22))),
                    )
                    .clicked()
            {
                *selected_path = Some(doc.path.clone());
            }
            if ui
                .add(
                    egui::Button::new(super::super::super::presentation::title_text(
                        ui,
                        super::super::super::presentation::human_title(&doc.title, &doc.path),
                    ))
                    .frame(false)
                    .wrap(),
                )
                .on_hover_text(super::super::super::presentation::human_title(
                    &doc.title, &doc.path,
                ))
                .clicked()
            {
                *selected_path = Some(doc.path.clone());
            }
            super::super::super::presentation::description(ui, &doc.text);
            let implementation = s.implementation_state(&doc.path);
            if implementation.is_some_and(|record| {
                matches!(
                    record.status,
                    crate::core::implementation::ImplementationStatus::AwaitingApproval
                        | crate::core::implementation::ImplementationStatus::ReadyToPublish
                ) && record.pr_url.is_none()
            }) {
                ui.colored_label(
                    theme::BLUE_BRIGHT,
                    "Implementation complete · approval required",
                );
                ui.horizontal(|ui| {
                    if ui.button("Approve").clicked() {
                        s.dispatch(ApplicationCommand::ApprovePublication {
                            ticket: doc.path.clone(),
                        });
                    }
                    if ui.button("Request changes").clicked() {
                        s.dispatch(ApplicationCommand::RequestPublicationChanges {
                            ticket: doc.path.clone(),
                        });
                        *selected_path = Some(doc.path.clone());
                    }
                });
            } else if implementation.is_some_and(|record| {
                record.status == crate::core::implementation::ImplementationStatus::ChangesRequested
            }) {
                ui.colored_label(theme::WARNING, "Changes requested · implementation paused");
            }
            let mut checklist =
                crate::ui::task_checklist::from_task(&doc.text, s.implementation_state(&doc.path));
            if let Some(progress) = s.task_progress(&doc.path) {
                for index in &progress.checklist {
                    if let Some(item) = checklist.get_mut(*index) {
                        item.complete = true;
                    }
                }
            }
            let elapsed = s.implementation_elapsed(&doc.path);
            if active {
                ui.horizontal(|ui| {
                    theme::operation_indicator(ui);
                    ui.add(
                        egui::Label::new(
                            RichText::new("Kool.ad/e is working")
                                .size(12.0)
                                .strong()
                                .color(theme::BLUE_BRIGHT),
                        )
                        .wrap(),
                    );
                });
                if let Some(elapsed) = &elapsed {
                    ui.label(
                        RichText::new(format!("{elapsed} elapsed"))
                            .size(11.0)
                            .color(theme::TEXT_DIM),
                    );
                }
            }
            task_cards::paint_task_failure(ui, s, &doc.path);
            crate::ui::task_checklist::paint(
                ui,
                &checklist,
                complete,
                true,
                active,
                elapsed.as_deref(),
            );
            if task_cards::task_conversation(ui, s, board, &doc.path, false) {
                *selected_path = Some(doc.path.clone());
            }
            if column == 4 && ui.small_button("Archive").clicked() {
                s.dispatch(ApplicationCommand::ArchiveTask {
                    ticket: doc.path.clone(),
                });
            }
            if active {
                let samples = s.activity_samples(Some(&doc.path));
                task_card_activity_band(
                    ui,
                    &samples,
                    true,
                    s.task_progress(&doc.path),
                    chrono::Utc::now().timestamp_millis(),
                );
            }
        },
    );
}

fn dependency_blocker(
    s: &dyn Surface,
    board: &crate::ui::planning_board::ViewModel,
    doc: &crate::artifacts::task_docs::TaskDocument,
) -> Option<String> {
    if doc.metadata_error.is_some() {
        return None;
    }
    let dependencies = if let Some(metadata) = &doc.metadata {
        metadata
            .dependency_uids
            .iter()
            .map(|uid| {
                board.task_documents.iter().find(|candidate| {
                    candidate
                        .identity
                        .as_ref()
                        .is_some_and(|identity| &identity.uid == uid)
                })
            })
            .collect::<Vec<_>>()
    } else {
        let filenames = crate::artifacts::task_docs::legacy_dependencies(&doc.text).ok()?;
        let parent = std::path::Path::new(&doc.path).parent()?.to_string_lossy();
        filenames
            .iter()
            .map(|filename| {
                let path = format!("{parent}/{filename}");
                board
                    .task_documents
                    .iter()
                    .find(|candidate| candidate.path == path)
            })
            .collect::<Vec<_>>()
    };
    for dependency in dependencies {
        let Some(dependency) = dependency else {
            return Some("a missing task dependency".into());
        };
        let complete = s
            .implementation_state(&dependency.path)
            .is_some_and(|state| {
                state.status == crate::core::implementation::ImplementationStatus::Completed
                    || state.pr_state == Some(crate::core::implementation::PullRequestState::Merged)
            });
        if !complete {
            return Some(dependency.title.clone());
        }
    }
    None
}
