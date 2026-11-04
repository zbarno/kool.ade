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
    let child_summary = doc.metadata.as_ref().and_then(|metadata| {
        let uid = metadata.uid.as_str();
        let children = board
            .task_documents
            .iter()
            .filter(|candidate| {
                candidate
                    .metadata
                    .as_ref()
                    .and_then(|metadata| metadata.parent_task_uid.as_deref())
                    == Some(uid)
            })
            .collect::<Vec<_>>();
        (!children.is_empty()).then(|| {
            let complete = children
                .iter()
                .filter(|child| {
                    board.lifecycle_tasks.get(&child.path)
                        == Some(&crate::core::lifecycle::TaskPhase::Done)
                        || s.implementation_state(&child.path).is_some_and(|state| {
                            state.status
                                == crate::core::implementation::ImplementationStatus::Completed
                        })
                })
                .count();
            let blocked = children
                .iter()
                .filter(|child| {
                    board.lifecycle_tasks.get(&child.path)
                        == Some(&crate::core::lifecycle::TaskPhase::Blocked)
                        || dependency_blocker(s, board, child).is_some()
                })
                .count();
            if blocked == 0 {
                format!("{complete} of {} subtasks complete", children.len())
            } else {
                format!(
                    "{complete} of {} subtasks complete · {blocked} blocked",
                    children.len()
                )
            }
        })
    });
    let pr_status = (lifecycle_phase != Some(crate::core::lifecycle::TaskPhase::Done))
        .then(|| board.lifecycle_pr_statuses.get(&doc.path).copied())
        .flatten();
    let lifecycle_record = doc.identity.as_ref().and_then(|identity| {
        let mut records = board
            .lifecycle_feature_tasks
            .iter()
            .filter(|task| task.planning_item_uid.as_deref() == Some(identity.uid.as_str()));
        let record = records.next()?;
        records.next().is_none().then_some(record)
    });
    let gate_track = super::task_semantics::gate_track(
        lifecycle_record,
        board.lifecycle_pr_statuses.get(&doc.path).copied(),
    );
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
        "Kool.ad/e is working on this task".to_owned()
    } else if let Some(prerequisite) = prerequisite.as_deref() {
        prerequisite.to_owned()
    } else if waiting {
        "Queued · implementation slots are full".to_owned()
    } else if needs_input.is_some() {
        "Waiting for your decision".to_owned()
    } else {
        lifecycle_phase.map_or_else(
            || {
                super::task_semantics::lifecycle_unavailable_status(
                    s.implementation_state(&doc.path).map(|state| state.status),
                )
            },
            |phase| super::task_semantics::next_step(phase, pr_status).to_owned(),
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
                if lifecycle_phase.is_none() && pr_status.is_none() {
                    theme::badge(
                        ui,
                        "Lifecycle not recorded",
                        theme::WARNING.gamma_multiply(0.16),
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
            if let Some(track) = &gate_track {
                ui.label(theme::helper_text(track));
            }
            if let Some(summary) = &child_summary {
                ui.label(theme::helper_text(summary));
            }
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
        return Some("current task metadata is missing".into());
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
