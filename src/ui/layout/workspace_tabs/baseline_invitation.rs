use crate::ui::Surface;

const BASELINE_PROMPT: &str = "Help me establish an evidence-grounded product baseline. Capture my intent about the project's purpose, users, desired outcome, initial scope and important constraints. For an existing codebase, use the bounded repository evidence supplied for this turn before asking technical questions; cite exact repository, ref, commit SHA, paths and tests inspected. Separate observed behavior, my confirmed intent, proposed scope and unknowns. Do not infer absence from incomplete docs or claim QA, merge or deployment without evidence. Ask only the next few high-value questions and save proposals as a private draft.";
const REVIEW_REQUESTED: &str = "koolade_baseline_evidence_review_requested";
const REVIEW_EVIDENCE: &str = "koolade_baseline_evidence_review_id";

pub(super) fn paint(ui: &mut egui::Ui, surface: &mut dyn Surface, compact: bool) {
    ui.group(|ui| {
        ui.set_max_width(if compact { 720.0 } else { 980.0 });
        ui.heading(if surface.has_published_product_spec() {
            "Review the product baseline"
        } else {
            "Let's establish a product baseline"
        });
        if surface.local_spec_draft().is_some() {
            ui.label("A private draft is saved. Published product documents remain unchanged until you review and authorize a separate publication.");
        } else if surface.has_published_product_spec() {
            ui.label("Check what the project is meant to do against current repository evidence. Proposed edits stay private until you review them.");
        } else {
            ui.label("Describe the idea or ask Man.ager to inspect an existing codebase. We'll record what is confirmed, what evidence shows, and what remains unknown.");
        }
        if review_pending(ui.ctx()) {
            paint_review_gate(ui, surface);
            return;
        }
        ui.horizontal_wrapped(|ui| {
            if ui.button("Start baseline interview").clicked() {
                start_interview(ui, surface);
            }
            let available = surface.repository_git_options().into_iter()
                .filter(|repository| repository.unavailable_reason.is_none())
                .collect::<Vec<_>>();
            let multiple_repositories = available.len() > 1;
            for repository in available {
                let label = if !multiple_repositories {
                    "Review this codebase".to_owned()
                } else {
                    format!("Review {}", repository.label)
                };
                if ui.button(label).clicked() {
                    let review_id = uuid::Uuid::new_v4().hyphenated().to_string();
                    ui.ctx().data_mut(|data| {
                        data.insert_temp(egui::Id::new(REVIEW_REQUESTED), review_id.clone());
                        data.insert_temp(egui::Id::new(REVIEW_EVIDENCE), String::new());
                        data.insert_temp(egui::Id::new("koolade_document_tab"), 2_u8);
                    });
                    surface.dispatch(crate::ui::ApplicationCommand::RequestProductBaselineEvidence {
                        review_id,
                        repository_id: repository.id,
                    });
                }
            }
        });
    });
    ui.add_space(8.0);
}

pub(super) fn review_pending(ctx: &egui::Context) -> bool {
    ctx.data_mut(|data| {
        data.get_temp::<String>(egui::Id::new(REVIEW_REQUESTED))
            .is_some()
    })
}

fn paint_review_gate(ui: &mut egui::Ui, surface: &mut dyn Surface) {
    if surface.manager_running() {
        ui.spinner();
        ui.label(
            "Man.ager is reviewing repository evidence. Findings will appear in Project activity.",
        );
        return;
    }
    let review_id = ui
        .ctx()
        .data_mut(|data| data.get_temp::<String>(egui::Id::new(REVIEW_REQUESTED)));
    let Some(review_id) = review_id else { return };
    let history = surface.manager_activity();
    let review_result = history
        .iter()
        .find(|message| message.baseline_review_id.as_deref() == Some(review_id.as_str()));
    let Some(review_result) = review_result else {
        ui.label("The evidence review is queued. Wait for Man.ager to start it, then inspect the result in Project activity.");
        return;
    };
    let failed = review_result.role == crate::domain::ChatRole::System;
    ui.label(if failed {
        "Man.ager could not complete this evidence review. You can continue with the available evidence, and unknowns will remain unconfirmed."
    } else {
        "Review Man.ager's findings in Project activity, then continue with a private baseline draft."
    });
    if ui
        .button(if failed {
            "Continue without repository evidence"
        } else {
            "I've reviewed the findings — continue"
        })
        .clicked()
    {
        ui.ctx()
            .data_mut(|data| data.remove::<String>(egui::Id::new(REVIEW_REQUESTED)));
        ui.ctx().data_mut(|data| {
            data.insert_temp(egui::Id::new(REVIEW_EVIDENCE), review_id.clone());
            data.insert_temp(egui::Id::new("koolade_document_tab"), 0_u8);
            data.insert_temp(egui::Id::new("koolade_spec_chat_collapsed"), false);
            data.insert_temp(egui::Id::new("koolade_spec_chat_intent"), 1_u8);
        });
        let draft = surface.chat_draft();
        if draft.trim().is_empty() {
            draft.push_str(BASELINE_PROMPT);
        }
    }
}

fn start_interview(ui: &egui::Ui, surface: &mut dyn Surface) {
    ui.ctx().data_mut(|data| {
        data.insert_temp(egui::Id::new("koolade_document_tab"), 0_u8);
        data.insert_temp(egui::Id::new("koolade_spec_chat_collapsed"), false);
        data.insert_temp(egui::Id::new("koolade_spec_chat_intent"), 1_u8);
    });
    let draft = surface.chat_draft();
    if draft.trim().is_empty() {
        draft.push_str(BASELINE_PROMPT);
    }
}
