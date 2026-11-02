use super::DlgProjectSettings;
use crate::{AppError, app::session::Project};

pub(super) fn load(project: &Project, draft: &mut DlgProjectSettings) {
    match crate::persistence::project_guidance::load_receipt_for_root(&project.state.repo_root) {
        Ok(Some(receipt)) => {
            draft.agent_guidance.clone_from(&receipt.content);
            draft.agent_guidance_enabled = receipt.enabled;
            draft.saved_agent_guidance = Some(receipt);
        }
        Ok(None) => {}
        Err(error) => {
            draft.agent_guidance_error = Some(error.to_string());
        }
    }
}

pub(super) fn apply(project: &Project, draft: &mut DlgProjectSettings) -> Result<bool, AppError> {
    let saved_content = draft
        .saved_agent_guidance
        .as_ref()
        .map_or("", |saved| saved.content.as_str());
    let changed_text = saved_content != draft.agent_guidance;
    let saved_enabled = draft
        .saved_agent_guidance
        .as_ref()
        .is_some_and(|saved| saved.enabled);
    let changed_enabled = saved_enabled != draft.agent_guidance_enabled;
    if !changed_text && !changed_enabled && draft.agent_guidance_error.is_none() {
        return Ok(false);
    }
    if changed_text && !draft.agent_guidance.trim().is_empty() && !draft.approve_agent_guidance {
        return Err(AppError::Other(
            "Review the full project guidance and explicitly approve the changed text before saving."
                .into(),
        ));
    }
    if draft.agent_guidance.len() > crate::persistence::project_guidance::MAX_GUIDANCE_BYTES {
        return Err(AppError::Other(
            "Project guidance exceeds the 16 KiB limit. Shorten it before approval.".into(),
        ));
    }

    let enabled = if changed_text && !draft.agent_guidance.trim().is_empty() {
        true
    } else if draft.agent_guidance.trim().is_empty() {
        false
    } else {
        draft.agent_guidance_enabled
    };
    let receipt = crate::persistence::project_guidance::save_for_root(
        &project.state.repo_root,
        &draft.agent_guidance,
        enabled,
    )
    .map_err(|error| {
        AppError::Other(format!("Could not save approved project guidance: {error}"))
    })?;
    draft.saved_agent_guidance = receipt;
    draft.agent_guidance_enabled = enabled;
    draft.approve_agent_guidance = false;
    draft.agent_guidance_error = None;
    draft.agent_guidance_saved = true;
    Ok(true)
}

pub(super) fn paint(ui: &mut egui::Ui, draft: &mut DlgProjectSettings) {
    ui.add_space(12.0);
    ui.label(crate::ui::theme::section_heading(
        "Approved project guidance",
    ));
    ui.label("Add concise conventions that the selected coding tools may use as project context. Kool.ad/e does not read repository instruction files automatically. This text cannot grant tools, change task scope, or override execution policy.");
    let response = ui.add_sized(
        [ui.available_width(), 180.0],
        egui::TextEdit::multiline(&mut draft.agent_guidance)
            .desired_rows(8)
            .char_limit(crate::persistence::project_guidance::MAX_GUIDANCE_BYTES),
    );
    if response.changed() {
        draft.approve_agent_guidance = false;
        draft.agent_guidance_enabled = false;
    }
    let changed = draft
        .saved_agent_guidance
        .as_ref()
        .is_none_or(|saved| saved.content != draft.agent_guidance);
    let too_large =
        draft.agent_guidance.len() > crate::persistence::project_guidance::MAX_GUIDANCE_BYTES;
    if changed && !draft.agent_guidance.trim().is_empty() {
        ui.checkbox(
            &mut draft.approve_agent_guidance,
            "Approve and use this exact guidance after reviewing the full text above",
        );
        ui.label("Approval is bound to the current project and exact text hash. Editing it requires approval again.");
    } else if draft.saved_agent_guidance.is_some() && !draft.agent_guidance.trim().is_empty() {
        ui.checkbox(
            &mut draft.agent_guidance_enabled,
            "Use the approved guidance in project task prompts",
        );
        ui.label("You can disable or re-enable this exact approved text at any time.");
    }
    if too_large {
        ui.colored_label(
            egui::Color32::LIGHT_RED,
            "Guidance exceeds the 16 KiB limit.",
        );
    }
    if let Some(error) = &draft.agent_guidance_error {
        ui.colored_label(
            egui::Color32::LIGHT_RED,
            format!("Saved guidance is invalid and remains disabled: {error}"),
        );
    }
    if draft.agent_guidance_saved {
        ui.label("Project guidance saved in private local settings.");
    }
}
