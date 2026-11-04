use super::*;

pub(super) fn paint(ui: &mut egui::Ui, surface: &dyn Surface) -> Option<String> {
    let draft = surface.local_spec_draft()?;
    if draft.changes.is_empty() {
        return None;
    }
    let selector_id = egui::Id::new("koolade_selected_local_spec_proposal");
    let mut selected = ui
        .ctx()
        .data_mut(|data| {
            data.get_temp::<Option<String>>(selector_id)
                .flatten()
                .or_else(|| data.get_persisted::<Option<String>>(selector_id).flatten())
        })
        .filter(|id| draft.changes.iter().any(|change| &change.document_id == id))
        .or_else(|| {
            draft
                .changes
                .first()
                .map(|change| change.document_id.clone())
        });
    egui::ComboBox::from_id_salt("local_spec_proposal_selector")
        .selected_text(selected.as_deref().unwrap_or("Select proposal"))
        .width(ui.available_width().min(400.0))
        .wrap_mode(egui::TextWrapMode::Truncate)
        .show_ui(ui, |ui| {
            for change in &draft.changes {
                let title = change
                    .content
                    .lines()
                    .find_map(|line| line.strip_prefix("# "))
                    .unwrap_or(&change.document_id);
                ui.selectable_value(&mut selected, Some(change.document_id.clone()), title);
            }
        });
    ui.ctx().data_mut(|data| {
        data.insert_persisted(selector_id, selected.clone());
        data.insert_temp(selector_id, selected.clone());
    });
    let matches = draft.base_published_spec_pin.as_ref() == surface.published_spec_pin();
    if matches {
        theme::badge(
            ui,
            "Local · not submitted",
            theme::ACCENT_SOFT,
            theme::ACCENT,
        );
    } else {
        theme::badge(
            ui,
            "Stale · review against published revision",
            theme::CHIP_FILL,
            theme::WARNING,
        );
    }
    ui.label(
        RichText::new(format!(
            "Base {} · proposal {} · PR status: not submitted",
            draft
                .base_published_spec_pin
                .as_ref()
                .map(|pin| pin.commit_sha.as_str())
                .unwrap_or("none"),
            draft.proposal_sha256
        ))
        .size(11.0)
        .weak(),
    );
    selected
        .as_deref()
        .and_then(|id| draft.changes.iter().find(|change| change.document_id == id))
        .map(|change| change.content.clone())
}
