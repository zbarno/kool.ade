use super::*;

#[path = "input/tests.rs"]
#[cfg(test)]
mod tests;

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
    let image_size = viewer_size(size, ui.available_width());
    let interactive =
        session.control_owner == crate::computer_use::mcp::ComputerUseControlOwnerV1::Human;
    let response = add_accessible_frame_image(ui, &texture, image_size, interactive);
    if !interactive {
        ui.label("Take control to use the mouse and keyboard.");
        return;
    }
    if let Some(position) = response.interact_pointer_pos()
        && (response.dragged() || response.clicked() || response.drag_stopped())
    {
        let local = position - response.rect.min;
        let [x, y] = guest_pointer_position(
            local,
            image_size,
            [session.frame_width, session.frame_height],
        );
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

fn add_accessible_frame_image(
    ui: &mut egui::Ui,
    texture: &egui::TextureHandle,
    image_size: egui::Vec2,
    interactive: bool,
) -> egui::Response {
    let sense = if interactive {
        egui::Sense::click_and_drag()
    } else {
        egui::Sense::hover()
    };
    let response = ui.add(
        egui::Image::new(texture)
            .fit_to_exact_size(image_size)
            .sense(sense),
    );
    response.widget_info(|| {
        egui::WidgetInfo::labeled(egui::WidgetType::Image, true, "Live VM desktop")
    });
    response
}

fn viewer_size(frame: [usize; 2], available_width: f32) -> egui::Vec2 {
    if frame[0] == 0 || frame[1] == 0 {
        return egui::Vec2::ZERO;
    }
    let available = available_width.max(1.0);
    let scale = (available / frame[0] as f32)
        .min(480.0 / frame[1] as f32)
        .min(1.0);
    egui::vec2(frame[0] as f32 * scale, frame[1] as f32 * scale)
}

fn guest_pointer_position(
    local: egui::Vec2,
    rendered: egui::Vec2,
    guest_frame: [u16; 2],
) -> [u16; 2] {
    if rendered.x <= 0.0 || rendered.y <= 0.0 || guest_frame[0] == 0 || guest_frame[1] == 0 {
        return [0, 0];
    }
    let x = (local.x / rendered.x * guest_frame[0] as f32)
        .round()
        .clamp(0.0, guest_frame[0].saturating_sub(1) as f32) as u16;
    let y = (local.y / rendered.y * guest_frame[1] as f32)
        .round()
        .clamp(0.0, guest_frame[1].saturating_sub(1) as f32) as u16;
    [x, y]
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
