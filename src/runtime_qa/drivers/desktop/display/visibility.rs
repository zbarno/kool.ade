use x11rb::{image::Image, protocol::xproto::Window, rust_connection::RustConnection};

use super::display_error;

pub(super) fn region_has_visible_content(
    connection: &RustConnection,
    window: Window,
    x: u16,
    y: u16,
    width: u16,
    height: u16,
) -> Result<bool, String> {
    if !(4..=256).contains(&width) || !(4..=256).contains(&height) {
        return Err("native visibility region exceeds the bounded image probe".into());
    }
    let x = i16::try_from(x).map_err(|_| "native visibility x coordinate is invalid")?;
    let y = i16::try_from(y).map_err(|_| "native visibility y coordinate is invalid")?;
    let image = Image::get(connection, window, x, y, width, height)
        .map_err(display_error)?
        .0;
    let mut colors = std::collections::HashMap::<u32, usize>::new();
    for py in 0..height {
        for px in 0..width {
            *colors.entry(image.get_pixel(px, py)).or_default() += 1;
        }
    }
    let Some(background) = colors.values().max().copied() else {
        return Ok(false);
    };
    let pixels = usize::from(width) * usize::from(height);
    Ok(colors.len() >= 2 && pixels.saturating_sub(background) * 100 >= pixels * 2)
}
