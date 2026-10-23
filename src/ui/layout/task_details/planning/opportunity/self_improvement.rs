use crate::{
    core::self_improvement::{
        SelfImprovementParentTaskProgressV1, SelfImprovementPhaseV1, SelfImprovementRunV1,
    },
    ui::{ApplicationCommand, Surface},
};
use egui::Ui;

#[cfg(test)]
#[path = "self_improvement/tests.rs"]
mod tests;

pub(super) fn paint_progress(ui: &mut Ui, surface: &mut dyn Surface, run: &SelfImprovementRunV1) {
    ui.collapsing("Self-improvement progress", |ui| {
        ui.label(format!("Current phase: {:?}", run.phase));
        let parent_count = run
            .parent_task_progress
            .len()
            .max(usize::from(run.owner_task_uid.is_some()));
        ui.label(format!("{parent_count} parent task(s)"));
        for (label, url) in pull_request_links(&run.parent_task_progress, run.pr_url.as_deref()) {
            ui.hyperlink_to(label, url);
        }
        if run.phase == SelfImprovementPhaseV1::MergedNotActivated {
            ui.colored_label(
                egui::Color32::YELLOW,
                "Merged; confirm after installing this build through the authorized update process",
            );
            if ui.button("Confirm installation").clicked() {
                surface.dispatch(ApplicationCommand::ConfirmSelfImprovementInstallation {
                    run_uid: run.run_uid.clone(),
                    expected_revision: run.revision,
                });
            }
        } else if run.phase == SelfImprovementPhaseV1::InstalledNotActivated {
            ui.colored_label(
                egui::Color32::YELLOW,
                "Installed; awaiting the app-owned capability readiness probe",
            );
        } else if run.phase == SelfImprovementPhaseV1::Activated {
            ui.label(format!(
                "Activated version: {}",
                run.activated_version.as_deref().unwrap_or("unknown")
            ));
        }
    });
}

fn pull_request_links(
    parents: &[SelfImprovementParentTaskProgressV1],
    fallback_url: Option<&str>,
) -> Vec<(String, String)> {
    let links = parents
        .iter()
        .filter_map(|parent| {
            parent.pr_url.as_ref().map(|url| {
                (
                    format!(
                        "Open PR for task {}",
                        parent.task_uid.get(..8).unwrap_or("unknown")
                    ),
                    url.clone(),
                )
            })
        })
        .collect::<Vec<_>>();
    let mut links = links;
    if let Some(url) = fallback_url
        && links.iter().all(|(_, parent_url)| parent_url != url)
    {
        links.push(("Open pull request".into(), url.to_owned()));
    }
    links
}
