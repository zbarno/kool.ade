use crate::ui::{Surface, task_detail::LifecycleView, theme};

const PAGE_SIZE: usize = 12;

#[derive(Clone)]
struct GalleryState {
    screenshots: Vec<(String, String, egui::TextureHandle)>,
    total: usize,
    offset: usize,
    error: Option<String>,
}

pub(super) fn paint(
    ui: &mut egui::Ui,
    surface: &mut Option<&mut dyn Surface>,
    ticket: &str,
    lifecycle: &LifecycleView,
) {
    let Some(run_id) = lifecycle.runtime_qa_run_id.as_deref() else {
        return;
    };
    egui::CollapsingHeader::new("Runtime QA screenshot gallery")
        .default_open(false)
        .show(ui, |ui| {
            let cache_id = egui::Id::new(("runtime_qa_gallery", run_id));
            let mut state = ui
                .ctx()
                .data_mut(|data| data.get_temp::<GalleryState>(cache_id));
            if state.is_none() {
                if ui.button("Load validated screenshots").clicked() {
                    state = Some(load(ui, surface, ticket, run_id, 0));
                    ui.ctx()
                        .data_mut(|data| data.insert_temp(cache_id, state.clone().unwrap()));
                } else {
                    ui.label(theme::helper_text(
                        "Screenshots are read from the completed, validated runtime QA run.",
                    ));
                }
            }
            if let Some(mut state) = state {
                if let Some(error) = &state.error {
                    ui.colored_label(theme::WARNING, error);
                    if ui.button("Retry loading screenshots").clicked() {
                        state = load(ui, surface, ticket, run_id, state.offset);
                        ui.ctx()
                            .data_mut(|data| data.insert_temp(cache_id, state.clone()));
                    }
                }
                if state.screenshots.is_empty() && state.error.is_none() {
                    ui.label("This QA run did not record screenshots.");
                }
                for (id, scenario, texture) in &state.screenshots {
                    ui.group(|ui| {
                        ui.label(format!("Scenario {scenario} · screenshot {id}"));
                        ui.add(egui::Image::new(texture).max_width(ui.available_width()));
                    });
                }
                if state.total > PAGE_SIZE {
                    ui.horizontal(|ui| {
                        if ui
                            .add_enabled(state.offset > 0, egui::Button::new("Previous"))
                            .clicked()
                        {
                            state = load(
                                ui,
                                surface,
                                ticket,
                                run_id,
                                state.offset.saturating_sub(PAGE_SIZE),
                            );
                            ui.ctx()
                                .data_mut(|data| data.insert_temp(cache_id, state.clone()));
                        }
                        ui.label(format!(
                            "{}–{} of {}",
                            state.offset + 1,
                            (state.offset + state.screenshots.len()).min(state.total),
                            state.total
                        ));
                        if ui
                            .add_enabled(
                                state.offset + PAGE_SIZE < state.total,
                                egui::Button::new("Next"),
                            )
                            .clicked()
                        {
                            state = load(ui, surface, ticket, run_id, state.offset + PAGE_SIZE);
                            ui.ctx()
                                .data_mut(|data| data.insert_temp(cache_id, state.clone()));
                        }
                    });
                }
            }
        });
}

fn load(
    ui: &egui::Ui,
    surface: &mut Option<&mut dyn Surface>,
    ticket: &str,
    run_id: &str,
    offset: usize,
) -> GalleryState {
    let Some(surface) = surface.as_deref_mut() else {
        return GalleryState {
            screenshots: Vec::new(),
            total: 0,
            offset,
            error: Some("Runtime QA evidence cannot be loaded in this view.".into()),
        };
    };
    let gallery = match surface.load_runtime_qa_screenshots(ticket, run_id, offset, PAGE_SIZE) {
        Ok(screenshots) => screenshots,
        Err(error) => {
            return GalleryState {
                screenshots: Vec::new(),
                total: 0,
                offset,
                error: Some(error),
            };
        }
    };
    let mut textures = Vec::with_capacity(gallery.screenshots.len());
    for screenshot in gallery.screenshots {
        let color_image = match decode_thumbnail(&screenshot.bytes) {
            Ok(image) => image,
            Err(error) => {
                return GalleryState {
                    screenshots: Vec::new(),
                    total: gallery.total,
                    offset,
                    error: Some(error),
                };
            }
        };
        let texture = ui.ctx().load_texture(
            format!(
                "runtime-qa:{}:{}:{}",
                run_id, screenshot.id, screenshot.sha256
            ),
            color_image,
            egui::TextureOptions::LINEAR,
        );
        textures.push((screenshot.id, screenshot.scenario_id, texture));
    }
    GalleryState {
        screenshots: textures,
        total: gallery.total,
        offset,
        error: None,
    }
}

fn decode_thumbnail(bytes: &[u8]) -> Result<egui::ColorImage, String> {
    let image = image::load_from_memory(bytes)
        .map_err(|_| {
            "A validated runtime QA screenshot could not be decoded for display.".to_owned()
        })?
        .thumbnail(720, 480)
        .to_rgba8();
    let size = [image.width() as usize, image.height() as usize];
    Ok(egui::ColorImage::from_rgba_unmultiplied(
        size,
        image.as_raw(),
    ))
}

#[cfg(test)]
mod tests {
    use super::decode_thumbnail;

    #[test]
    fn screenshot_preview_is_bounded_to_desktop_gallery_dimensions() {
        let image = image::DynamicImage::ImageRgba8(image::RgbaImage::from_pixel(
            1440,
            900,
            image::Rgba([12, 24, 36, 255]),
        ));
        let mut bytes = std::io::Cursor::new(Vec::new());
        image.write_to(&mut bytes, image::ImageFormat::Png).unwrap();
        let preview = decode_thumbnail(bytes.get_ref()).unwrap();
        assert_eq!(preview.size, [720, 450]);
    }

    #[test]
    fn malformed_screenshot_preview_returns_an_actionable_error() {
        assert!(
            decode_thumbnail(b"not a png")
                .unwrap_err()
                .contains("could not be decoded")
        );
    }
}
