use std::{
    thread,
    time::{Duration, Instant},
};

use image::GenericImageView;

use super::super::super::{display::DesktopDisplay, process};

pub(super) fn wait_for_window(
    display: &DesktopDisplay,
    timeout: Duration,
    root_pid: u32,
) -> Result<super::super::super::display::WindowInfo, String> {
    let deadline = Instant::now() + timeout;
    loop {
        let owner_pids = process::descendants(root_pid);
        if let Some(window) = display
            .windows(&owner_pids)?
            .into_iter()
            .find(|window| window.title.contains("Ready"))
        {
            return Ok(window);
        }
        if Instant::now() >= deadline {
            return Err("packaged Chromium did not create an owned X11 window".into());
        }
        thread::sleep(Duration::from_millis(25));
    }
}

pub(super) fn wait_for_color(
    display: &DesktopDisplay,
    timeout: Duration,
    expected: (u8, u8, u8),
) -> Result<(u16, u16), String> {
    let deadline = Instant::now() + timeout;
    loop {
        let screenshot = display.screenshot_png()?;
        let image = image::load_from_memory(&screenshot)
            .map_err(|error| format!("could not inspect Chromium fixture screenshot: {error}"))?;
        let rgb = image.to_rgb8();
        if let Some((index, _)) = rgb
            .pixels()
            .enumerate()
            .find(|(_, pixel)| (pixel[0], pixel[1], pixel[2]) == expected)
        {
            let width = rgb.width() as usize;
            return Ok(((index % width) as u16, (index / width) as u16));
        }
        if Instant::now() >= deadline {
            return Err(
                "packaged Chromium fixture did not render the expected initial color".into(),
            );
        }
        thread::sleep(Duration::from_millis(50));
    }
}

pub(super) fn wait_for_pixel(
    display: &DesktopDisplay,
    timeout: Duration,
    point: (u16, u16),
    expected: (u8, u8, u8),
) -> Result<(), String> {
    let deadline = Instant::now() + timeout;
    loop {
        let screenshot = display.screenshot_png()?;
        let image = image::load_from_memory(&screenshot)
            .map_err(|error| format!("could not inspect Chromium fixture screenshot: {error}"))?;
        let pixel = image.get_pixel(u32::from(point.0), u32::from(point.1)).0;
        if (pixel[0], pixel[1], pixel[2]) == expected {
            return Ok(());
        }
        if Instant::now() >= deadline {
            return Err(
                "packaged Chromium fixture did not reach the expected rendered state".into(),
            );
        }
        thread::sleep(Duration::from_millis(50));
    }
}

pub(super) fn relative_coordinate(
    state_pixel: u16,
    window_origin: u16,
    state_origin: u16,
    target: u16,
) -> Result<u16, String> {
    let relative = i32::from(state_pixel) - i32::from(window_origin) - i32::from(state_origin)
        + i32::from(target);
    u16::try_from(relative).map_err(|_| "Chromium fixture control is outside its window".into())
}

pub(super) fn screenshot(name: &str, bytes: &[u8]) -> serde_json::Value {
    serde_json::json!({
        "name": name,
        "mime": "image/png",
        "sha256": super::process::sha256(bytes),
        "width": 1440,
        "height": 900,
        "bytes_base64": super::super::encoding::base64(bytes),
    })
}
