use super::{add_accessible_frame_image, guest_pointer_position, viewer_size};

// This headless egui frame checks the production widget's generated AccessKit
// semantics at compact and desktop widths; it needs no host VM or provider.
#[test]
fn live_viewer_has_a_screen_reader_label_at_compact_and_desktop_widths() {
    let context = egui::Context::default();
    context.enable_accesskit();
    let texture = context.load_texture(
        "computer-viewer-accessibility-test",
        egui::ColorImage::new([1, 1], vec![egui::Color32::WHITE]),
        egui::TextureOptions::LINEAR,
    );

    for width in [320.0, 1280.0] {
        let mut output = context.run_ui(
            egui::RawInput {
                screen_rect: Some(egui::Rect::from_min_size(
                    egui::Pos2::ZERO,
                    egui::vec2(width, 720.0),
                )),
                ..Default::default()
            },
            |ui| {
                let size = viewer_size([1280, 800], ui.available_width());
                let response = add_accessible_frame_image(ui, &texture, size, false);
                assert!(response.rect.width() <= width);
            },
        );
        let tree = output
            .platform_output
            .accesskit_update
            .expect("egui should publish an accessibility tree update");
        assert!(tree.nodes.iter().any(|(_, node)| {
            node.role() == egui::accesskit::Role::Image
                && node.label().is_some_and(|label| label == "Live VM desktop")
        }));
        output.textures_delta.clear();
    }
}

#[test]
fn viewer_fits_wide_and_narrow_viewports_without_upscaling() {
    let wide_guest = viewer_size([1280, 800], 640.0);
    assert_eq!(wide_guest, egui::vec2(640.0, 400.0));

    let narrow_tall_guest = viewer_size([1920, 1080], 1200.0);
    assert!((narrow_tall_guest.x - (1920.0 * 480.0 / 1080.0)).abs() < 0.01);
    assert_eq!(narrow_tall_guest.y, 480.0);

    let small_guest = viewer_size([640, 400], 1200.0);
    assert_eq!(small_guest, egui::vec2(640.0, 400.0));
}

#[test]
fn viewer_pointer_maps_scaled_coordinates_to_guest_pixels() {
    assert_eq!(
        guest_pointer_position(
            egui::vec2(320.0, 200.0),
            egui::vec2(640.0, 400.0),
            [1280, 800]
        ),
        [640, 400]
    );
    assert_eq!(
        guest_pointer_position(egui::vec2(0.0, 0.0), egui::vec2(640.0, 400.0), [1280, 800]),
        [0, 0]
    );
    assert_eq!(
        guest_pointer_position(
            egui::vec2(640.0, 400.0),
            egui::vec2(640.0, 400.0),
            [1280, 800]
        ),
        [1279, 799]
    );
}

#[test]
fn viewer_geometry_handles_empty_frames_and_outside_pointer_positions() {
    assert_eq!(viewer_size([0, 800], 640.0), egui::Vec2::ZERO);
    assert_eq!(
        guest_pointer_position(
            egui::vec2(-4.0, 404.0),
            egui::vec2(640.0, 400.0),
            [1280, 800]
        ),
        [0, 799]
    );
    assert_eq!(
        guest_pointer_position(egui::vec2(20.0, 20.0), egui::Vec2::ZERO, [1280, 800]),
        [0, 0]
    );
}
