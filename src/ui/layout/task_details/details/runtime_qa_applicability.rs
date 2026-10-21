use crate::ui::{Surface, task_detail};

pub(super) fn paint(
    ui: &mut egui::Ui,
    surface: &mut dyn Surface,
    ticket: &str,
    view: &task_detail::ViewModel,
) {
    if let Some(error) = view.runtime_qa_applicability_error.as_deref() {
        ui.colored_label(
            crate::ui::theme::WARNING,
            format!("Applicability review unavailable: {error}"),
        );
    }
    let Some(lifecycle) = view.lifecycle.as_ref() else {
        return;
    };
    if lifecycle.task_phase != crate::core::lifecycle::TaskPhase::AwaitingRuntimeQa
        || lifecycle.runtime_qa_applicability.is_some()
    {
        return;
    }
    let selected_headless = view.runtime_capability.as_ref().is_some_and(|runtime| {
        runtime.targets.iter().any(|target| {
            target.selected
                && target.app_surface == crate::project_runtime::AppSurface::HeadlessService
        })
    });
    if !selected_headless {
        return;
    }
    ui.group(|ui| {
        ui.label(crate::ui::theme::section_heading("Headless visual QA applicability"));
        ui.label("An independent reviewer can verify that visual QA does not apply to this selected service target. This decision never certifies API or service test coverage.");
        if let Some(proposal) = view.runtime_qa_applicability_proposal.as_ref() {
            ui.label(format!("Independent review · {} · task revision {}", proposal.proposal.assessment.reviewer_execution_id, proposal.proposal.task_revision));
            ui.label(format!("Reason · {}", proposal.proposal.assessment.reason));
            if proposal.stale {
                ui.colored_label(crate::ui::theme::WARNING, "The reviewed code, task revision, or approved specification changed. Generate a fresh assessment.");
                if !view.runtime_qa_applicability_reviewing
                    && ui.button("Start fresh applicability review").clicked()
                {
                    surface.dispatch(crate::ui::ApplicationCommand::ReviewRuntimeQaApplicability {
                        ticket: ticket.to_owned(),
                    });
                }
            } else if ui.button("Manager: record visual QA as not applicable").clicked() {
                surface.dispatch(crate::ui::ApplicationCommand::ApproveRuntimeQaApplicability {
                    ticket: ticket.to_owned(),
                    expected_task_revision: proposal.proposal.task_revision,
                    proposal_sha256: proposal.proposal_sha256.clone(),
                });
            }
        } else if view.runtime_qa_applicability_reviewing {
            ui.label("Independent applicability review is running.");
        } else if ui.button("Start independent applicability review").clicked() {
            surface.dispatch(crate::ui::ApplicationCommand::ReviewRuntimeQaApplicability {
                ticket: ticket.to_owned(),
            });
        }
    });
}
