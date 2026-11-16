use super::{guest_pointer_position, viewer_size};

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
