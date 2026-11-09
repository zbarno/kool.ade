use super::*;

pub(super) fn paint_frame(
    ui: &mut egui::Ui,
    surface: &mut dyn Surface,
    task_id: &str,
    session: &crate::computer_use::mcp::ComputerUseTaskSessionViewV1,
    png: &[u8],
) {
    let Ok(image) = image::load_from_memory(png) else {
        ui.colored_label(
            crate::ui::theme::WARNING,
            "The desktop frame could not be decoded.",
        );
        return;
    };
    let image = image.to_rgba8();
    let size = [image.width() as usize, image.height() as usize];
    let color = egui::ColorImage::from_rgba_unmultiplied(size, image.as_raw());
    let texture = ui.ctx().load_texture(
        format!("computer-frame-{}", session.session_id),
        color,
        egui::TextureOptions::LINEAR,
    );
    let available = ui.available_width().max(1.0);
    let scale = (available / size[0] as f32)
        .min(480.0 / size[1] as f32)
        .min(1.0);
    let image_size = egui::vec2(size[0] as f32 * scale, size[1] as f32 * scale);
    let interactive =
        session.control_owner == crate::computer_use::mcp::ComputerUseControlOwnerV1::Human;
    let sense = if interactive {
        egui::Sense::click_and_drag()
    } else {
        egui::Sense::hover()
    };
    let response = ui.add(
        egui::Image::new(&texture)
            .fit_to_exact_size(image_size)
            .sense(sense),
    );
    response.widget_info(|| {
        egui::WidgetInfo::labeled(egui::WidgetType::Image, true, "Live VM desktop")
    });
    if !interactive {
        ui.label("Take control to use the mouse and keyboard.");
        return;
    }
    if let Some(position) = response.interact_pointer_pos()
        && (response.dragged() || response.clicked() || response.drag_stopped())
    {
        let local = position - response.rect.min;
        let x = (local.x / image_size.x * session.frame_width as f32)
            .round()
            .clamp(0.0, session.frame_width.saturating_sub(1) as f32) as u16;
        let y = (local.y / image_size.y * session.frame_height as f32)
            .round()
            .clamp(0.0, session.frame_height.saturating_sub(1) as f32) as u16;
        let buttons = if response.dragged() || response.clicked() {
            1
        } else {
            0
        };
        send(
            surface,
            task_id,
            session,
            crate::ui::ComputerUseTaskActionV1::DisplayInput(
                crate::computer_use::ComputerDisplayInputV1::Pointer { x, y, buttons },
            ),
        );
        if response.clicked() {
            send(
                surface,
                task_id,
                session,
                crate::ui::ComputerUseTaskActionV1::DisplayInput(
                    crate::computer_use::ComputerDisplayInputV1::Pointer { x, y, buttons: 0 },
                ),
            );
        }
    }
    if response.has_focus() {
        forward_keyboard(ui, surface, task_id, session);
    } else if response.clicked() {
        response.request_focus();
    }
}

fn forward_keyboard(
    ui: &mut egui::Ui,
    surface: &mut dyn Surface,
    task_id: &str,
    session: &crate::computer_use::mcp::ComputerUseTaskSessionViewV1,
) {
    let events = ui.input(|input| input.events.clone());
    for event in events {
        match event {
            egui::Event::Text(text) if !text.is_empty() => {
                for chunk in text.chars().collect::<Vec<_>>().chunks(64) {
                    let text: String = chunk.iter().collect();
                    send(
                        surface,
                        task_id,
                        session,
                        crate::ui::ComputerUseTaskActionV1::DisplayInput(
                            crate::computer_use::ComputerDisplayInputV1::Text { text },
                        ),
                    );
                }
            }
            egui::Event::Key {
                key,
                pressed,
                repeat: false,
                ..
            } => {
                if let Some(keysym) = keysym(key) {
                    send(
                        surface,
                        task_id,
                        session,
                        crate::ui::ComputerUseTaskActionV1::DisplayInput(
                            crate::computer_use::ComputerDisplayInputV1::Key { keysym, pressed },
                        ),
                    );
                }
            }
            _ => {}
        }
    }
}

fn keysym(key: egui::Key) -> Option<u32> {
    Some(match key {
        egui::Key::Enter => 0xff0d,
        egui::Key::Escape => 0xff1b,
        egui::Key::Tab => 0xff09,
        egui::Key::Backspace => 0xff08,
        egui::Key::Delete => 0xffff,
        egui::Key::ArrowLeft => 0xff51,
        egui::Key::ArrowUp => 0xff52,
        egui::Key::ArrowRight => 0xff53,
        egui::Key::ArrowDown => 0xff54,
        egui::Key::Home => 0xff50,
        egui::Key::End => 0xff57,
        egui::Key::PageUp => 0xff55,
        egui::Key::PageDown => 0xff56,
        egui::Key::Space => 0x20,
        _ => return None,
    })
}

pub(super) fn button(
    ui: &mut egui::Ui,
    surface: &mut dyn Surface,
    task_id: &str,
    session: &crate::computer_use::mcp::ComputerUseTaskSessionViewV1,
    label: &str,
    action: crate::ui::ComputerUseTaskActionV1,
) {
    if ui
        .add_enabled(
            !session.control_transition_pending,
            egui::Button::new(label),
        )
        .clicked()
    {
        send(surface, task_id, session, action);
    }
}

pub(super) fn send(
    surface: &mut dyn Surface,
    task_id: &str,
    session: &crate::computer_use::mcp::ComputerUseTaskSessionViewV1,
    action: crate::ui::ComputerUseTaskActionV1,
) {
    surface.dispatch(crate::ui::ApplicationCommand::ComputerUseTask {
        task_id: task_id.to_owned(),
        session_id: session.session_id.clone(),
        action,
    });
}
