use super::application_icon;

#[test]
fn embedded_application_icon_decodes_to_a_square_rgba_image() {
    let icon = application_icon();

    assert_eq!((icon.width, icon.height), (128, 128));
    assert_eq!(icon.rgba.len(), 128 * 128 * 4);
}
