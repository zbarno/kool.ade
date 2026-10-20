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
    let waiting = s.implementation_waiting_for_capacity(&doc.path);
    let complete = super::task_card_column(s, board, doc) == 4;
    let prerequisite =
        dependency_blocker(s, board, doc).map(|title| format!("Waiting for {title}"));
    let title = task_cards::card_summary(super::super::super::presentation::human_title(
        &doc.title, &doc.path,
    ));
    let lifecycle_phase = board.lifecycle_tasks.get(&doc.path).copied();
    let pr_status = (lifecycle_phase != Some(crate::core::lifecycle::TaskPhase::Done))
        .then(|| board.lifecycle_pr_statuses.get(&doc.path).copied())
        .flatten();
    let needs_input = super::super::attention::linked_user_action(board, &doc.path);
    let pr_needs_user_action = matches!(
        pr_status,
        Some(
            crate::core::lifecycle::PullRequestPreparationStatus::AwaitingAuthorization
                | crate::core::lifecycle::PullRequestPreparationStatus::NeedsAttention
                | crate::core::lifecycle::PullRequestPreparationStatus::ClosedUnmerged
        )
    );
    let status = if active {
        "Kool.ad/e is working on this task"
    } else if let Some(prerequisite) = prerequisite.as_deref() {
        prerequisite
    } else if waiting {
        "Queued · implementation slots are full"
    } else if needs_input.is_some() {
        "Waiting for your decision"
    } else {
        lifecycle_phase.map_or_else(
            || match column {
                0 => "Ready for implementation",
                1 => "In progress",
                2 => "Ready for review",
                3 => "Needs attention",
                _ => "Completed",
            },
            |phase| super::task_semantics::next_step(phase, pr_status),
        )
    };
    let open_details =
        task_cards::board_card(ui, &doc.path, None, active, column == 3, complete, |ui| {
            ui.horizontal_wrapped(|ui| {
                theme::badge(
                    ui,
                    super::task_semantics::kind_label(doc, lifecycle_phase, pr_status),
                    theme::PANEL,
                    theme::TEXT_DIM,
                );
                if needs_input.is_some() || pr_needs_user_action {
                    super::super::attention::badge(
                        ui,
                        super::super::attention::Kind::WaitingOnUser,
                    );
                    ui.label(
                        RichText::new("Needs your input")
                            .strong()
                            .color(theme::WARNING),
                    );
                } else if column == 3 {
                    super::super::attention::badge(
                        ui,
                        super::super::attention::task_kind(s, &doc.path),
                    );
                }
                if let Some(phase) = lifecycle_phase {
                    theme::badge(
                        ui,
                        crate::ui::task_detail::task_phase_label(phase),
                        theme::ACCENT_SOFT,
                        theme::TEXT,
                    );
                }
                if let Some(status) = pr_status {
                    theme::badge(
                        ui,
                        super::task_semantics::pr_status_label(status),
                        theme::ACCENT_SOFT,
                        theme::TEXT,
                    );
                }
            });
            if let Some(item) = needs_input {
                ui.label(RichText::new(&item.question).strong().color(theme::TEXT));
            }
            if ui
                .add(
                    egui::Button::new(RichText::new(title).strong().size(15.0).color(theme::TEXT))
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
            ui.label(theme::helper_text(status));
            super::super::super::super::activity::paint_card(ui, s, &doc.path, true);
            let pr_action = pr_status.and_then(super::task_semantics::pr_action_label);
            if (column == 2 || column == 3 || pr_action.is_some())
                && ui
                    .add_sized(
                        [ui.available_width(), 28.0],
                        egui::Button::new(if needs_input.is_some() {
                            "Respond"
                        } else if let Some(action) = pr_action {
                            action
                        } else if column == 2 {
                            "Review"
                        } else {
                            "Open details"
                        })
                        .fill(if needs_input.is_some() {
                            theme::WARNING.gamma_multiply(0.18)
                        } else {
                            theme::ACCENT_SOFT
                        }),
                    )
                    .clicked()
            {
                *selected_path = Some(doc.path.clone());
            }
        });
    if open_details {
        *selected_path = Some(doc.path.clone());
    }
}

fn dependency_blocker(
    surface: &dyn Surface,
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
        let complete = surface
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
