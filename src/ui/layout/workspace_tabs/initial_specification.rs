use crate::ui::Surface;

pub(super) fn paint(ui: &mut egui::Ui, surface: &mut dyn Surface) {
    let Some(pin) = surface.published_spec_pin() else {
        ui.label("Connect a code repository with a published Git ref before initializing its specification.");
        return;
    };
    let documents =
        crate::core::spec_publication::initial_product_documents(surface.session_title());
    ui.separator();
    ui.heading("Initialize the published specification");
    ui.label(format!(
        "This code repository has no published product modules at {}. Review the starter documents below. Unknown details are labeled; this action opens an isolated source-repository PR and does not change the current checkout.",
        pin.commit_sha
    ));
    egui::ScrollArea::vertical()
        .max_height(360.0)
        .show(ui, |ui| {
            for document in &documents {
                let name = std::path::Path::new(&document.path)
                    .file_name()
                    .and_then(|name| name.to_str())
                    .unwrap_or(&document.path);
                egui::CollapsingHeader::new(name)
                    .id_salt(("initial-specification", &document.path))
                    .show(ui, |ui| {
                        crate::ui::markdown::paint(
                            ui,
                            &document.content,
                            crate::ui::markdown::CHAT,
                        );
                    });
            }
        });
    let consent_id = egui::Id::new("initial_specification_publish_consent");
    let mut consent = ui
        .ctx()
        .data_mut(|data| data.get_temp::<bool>(consent_id).unwrap_or(false));
    ui.checkbox(
        &mut consent,
        "I reviewed these documents and authorize opening their source-repository PR.",
    );
    ui.ctx()
        .data_mut(|data| data.insert_temp(consent_id, consent));
    if ui
        .add_enabled(consent, egui::Button::new("Open initial specification PR"))
        .clicked()
    {
        surface.dispatch(crate::ui::ApplicationCommand::PublishInitialProductSpecification);
        ui.ctx()
            .data_mut(|data| data.insert_temp(consent_id, false));
    }
}
