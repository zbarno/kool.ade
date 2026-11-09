//! Bounded RFB client for QEMU's private Unix display socket.
//!
//! This module deliberately has no TCP listener or caller supplied path. The
//! backend resolves the socket from its verified, app-owned VM directory.

use std::{
    io::{Read, Write},
    os::unix::{fs::FileTypeExt, net::UnixStream},
    path::Path,
    time::Duration,
};

use super::super::types::{ComputerDisplayFrameV1, ComputerDisplayInputV1};
use crate::computer_use::ComputerBackendErrorV1 as Error;

const MAX_WIDTH: u16 = 1_920;
const MAX_HEIGHT: u16 = 1_080;
const MAX_FRAME_BYTES: usize = MAX_WIDTH as usize * MAX_HEIGHT as usize * 4;
const CLIENT_VERSION: &[u8; 12] = b"RFB 003.008\n";

pub(super) fn capture(path: &Path) -> Result<ComputerDisplayFrameV1, Error> {
    let mut stream = connect(path)?;
    let (width, height) = negotiate(&mut stream)?;
    if !valid_dimensions(width, height) {
        return Err(Error::ResourceLimit);
    }
    stream.write_all(&[0, 0, 0, 0]).map_err(|_| Error::Io)?; // SetPixelFormat
    stream.write_all(&[32, 24, 0, 1]).map_err(|_| Error::Io)?;
    stream
        .write_all(&[0, 255, 0, 255, 0, 255, 16, 8, 0, 0, 0, 0])
        .map_err(|_| Error::Io)?;
    stream
        .write_all(&[2, 0, 0, 1, 0, 0, 0, 0])
        .map_err(|_| Error::Io)?; // raw encoding
    stream.write_all(&[3, 0]).map_err(|_| Error::Io)?;
    stream
        .write_all(&0_u16.to_be_bytes())
        .map_err(|_| Error::Io)?;
    stream
        .write_all(&0_u16.to_be_bytes())
        .map_err(|_| Error::Io)?;
    stream
        .write_all(&width.to_be_bytes())
        .map_err(|_| Error::Io)?;
    stream
        .write_all(&height.to_be_bytes())
        .map_err(|_| Error::Io)?;

    // QEMU may send an initial update before reading our request. Ignore only
    // bounded server messages we explicitly support; never allocate from an
    // unvalidated rectangle size.
    let message = read_u8(&mut stream)?;
    if message != 0 {
        return Err(Error::GuestChannelUnavailable);
    }
    let _padding = read_u8(&mut stream)?;
    let rectangles = read_u16(&mut stream)?;
    if rectangles == 0 || rectangles > 64 {
        return Err(Error::GuestChannelUnavailable);
    }
    let mut rgba = vec![0_u8; width as usize * height as usize * 4];
    for _ in 0..rectangles {
        let x = read_u16(&mut stream)?;
        let y = read_u16(&mut stream)?;
        let rect_width = read_u16(&mut stream)?;
        let rect_height = read_u16(&mut stream)?;
        let encoding = read_i32(&mut stream)?;
        if encoding != 0
            || rect_width == 0
            || rect_height == 0
            || x.saturating_add(rect_width) > width
            || y.saturating_add(rect_height) > height
        {
            return Err(Error::GuestChannelUnavailable);
        }
        let byte_count = rect_width as usize * rect_height as usize * 4;
        if byte_count > MAX_FRAME_BYTES {
            return Err(Error::ResourceLimit);
        }
        let mut pixels = vec![0; byte_count];
        stream.read_exact(&mut pixels).map_err(|_| Error::Io)?;
        for row in 0..rect_height as usize {
            for column in 0..rect_width as usize {
                let src = (row * rect_width as usize + column) * 4;
                let dst = ((y as usize + row) * width as usize + x as usize + column) * 4;
                rgba[dst..dst + 4].copy_from_slice(&[
                    pixels[src + 2],
                    pixels[src + 1],
                    pixels[src],
                    255,
                ]);
            }
        }
    }
    let image = image::RgbaImage::from_raw(width as u32, height as u32, rgba)
        .ok_or(Error::GuestChannelUnavailable)?;
    let mut png = Vec::new();
    image::DynamicImage::ImageRgba8(image)
        .write_to(&mut std::io::Cursor::new(&mut png), image::ImageFormat::Png)
        .map_err(|_| Error::Io)?;
    if png.len() > crate::computer_use::guest_protocol::MAX_SCREENSHOT_PNG_BYTES {
        return Err(Error::ResourceLimit);
    }
    Ok(ComputerDisplayFrameV1 { width, height, png })
}

pub(super) fn send_input(path: &Path, input: &ComputerDisplayInputV1) -> Result<(), Error> {
    let mut stream = connect(path)?;
    let (width, height) = negotiate(&mut stream)?;
    match input {
        ComputerDisplayInputV1::Pointer { x, y, buttons } if *buttons & !0b1_1111 == 0 => {
            if *x >= width || *y >= height {
                return Err(Error::InvalidSession);
            }
            stream.write_all(&[5, *buttons]).map_err(|_| Error::Io)?;
            stream.write_all(&x.to_be_bytes()).map_err(|_| Error::Io)?;
            stream.write_all(&y.to_be_bytes()).map_err(|_| Error::Io)?;
        }
        ComputerDisplayInputV1::Key { keysym, pressed } if valid_keysym(*keysym) => {
            stream
                .write_all(&[4, u8::from(*pressed), 0, 0])
                .map_err(|_| Error::Io)?;
            stream
                .write_all(&keysym.to_be_bytes())
                .map_err(|_| Error::Io)?;
        }
        ComputerDisplayInputV1::Text { text } if !text.is_empty() && text.chars().count() <= 64 => {
            for character in text.chars() {
                let codepoint = character as u32;
                let keysym = if (0x20..=0x7e).contains(&codepoint) {
                    codepoint
                } else {
                    0x0100_0000 | codepoint
                };
                if !valid_keysym(keysym) {
                    return Err(Error::InvalidSession);
                }
                stream.write_all(&[4, 1, 0, 0]).map_err(|_| Error::Io)?;
                stream
                    .write_all(&keysym.to_be_bytes())
                    .map_err(|_| Error::Io)?;
                stream.write_all(&[4, 0, 0, 0]).map_err(|_| Error::Io)?;
                stream
                    .write_all(&keysym.to_be_bytes())
                    .map_err(|_| Error::Io)?;
            }
        }
        _ => return Err(Error::InvalidSession),
    }
    stream.flush().map_err(|_| Error::Io)
}

fn connect(path: &Path) -> Result<UnixStream, Error> {
    let metadata = std::fs::symlink_metadata(path).map_err(|_| Error::GuestChannelUnavailable)?;
    if !metadata.file_type().is_socket() {
        return Err(Error::OwnershipMismatch);
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::{MetadataExt, PermissionsExt};
        let parent = std::fs::symlink_metadata(path.parent().ok_or(Error::OwnershipMismatch)?)
            .map_err(|_| Error::OwnershipMismatch)?;
        if metadata.uid() != unsafe { libc::geteuid() }
            || !parent.file_type().is_dir()
            || parent.uid() != unsafe { libc::geteuid() }
            || parent.permissions().mode() & 0o077 != 0
        {
            return Err(Error::OwnershipMismatch);
        }
    }
    let stream = UnixStream::connect(path).map_err(|_| Error::GuestChannelUnavailable)?;
    stream
        .set_read_timeout(Some(Duration::from_secs(2)))
        .map_err(|_| Error::Io)?;
    stream
        .set_write_timeout(Some(Duration::from_secs(2)))
        .map_err(|_| Error::Io)?;
    Ok(stream)
}

fn negotiate(stream: &mut UnixStream) -> Result<(u16, u16), Error> {
    let version = read_array::<12>(stream)?;
    if &version != CLIENT_VERSION {
        return Err(Error::GuestChannelUnavailable);
    }
    stream.write_all(CLIENT_VERSION).map_err(|_| Error::Io)?;
    let count = read_u8(stream)?;
    if count == 0 || count > 32 {
        return Err(Error::GuestChannelUnavailable);
    }
    let mut security = vec![0; count as usize];
    stream.read_exact(&mut security).map_err(|_| Error::Io)?;
    if !security.contains(&1) {
        return Err(Error::GuestChannelUnavailable);
    }
    stream.write_all(&[1]).map_err(|_| Error::Io)?;
    if read_u32(stream)? != 0 {
        return Err(Error::GuestChannelUnavailable);
    }
    stream.write_all(&[1]).map_err(|_| Error::Io)?; // shared desktop
    let width = read_u16(stream)?;
    let height = read_u16(stream)?;
    let _pixel_format = read_array::<16>(stream)?;
    let name_len = read_u32(stream)? as usize;
    if name_len > 4_096 {
        return Err(Error::ResourceLimit);
    }
    let mut name = vec![0; name_len];
    stream.read_exact(&mut name).map_err(|_| Error::Io)?;
    Ok((width, height))
}

fn valid_dimensions(width: u16, height: u16) -> bool {
    width > 0 && height > 0 && width <= MAX_WIDTH && height <= MAX_HEIGHT
}

fn valid_keysym(value: u32) -> bool {
    (0x20..=0x7e).contains(&value)
        || (0xff00..=0xffff).contains(&value)
        || (0x0100_0000..=0x0110_ffff).contains(&value)
}

fn read_u8(stream: &mut UnixStream) -> Result<u8, Error> {
    let mut value = [0; 1];
    stream.read_exact(&mut value).map_err(|_| Error::Io)?;
    Ok(value[0])
}
fn read_u16(stream: &mut UnixStream) -> Result<u16, Error> {
    let mut value = [0; 2];
    stream.read_exact(&mut value).map_err(|_| Error::Io)?;
    Ok(u16::from_be_bytes(value))
}
fn read_u32(stream: &mut UnixStream) -> Result<u32, Error> {
    let mut value = [0; 4];
    stream.read_exact(&mut value).map_err(|_| Error::Io)?;
    Ok(u32::from_be_bytes(value))
}
fn read_i32(stream: &mut UnixStream) -> Result<i32, Error> {
    let mut value = [0; 4];
    stream.read_exact(&mut value).map_err(|_| Error::Io)?;
    Ok(i32::from_be_bytes(value))
}
fn read_array<const N: usize>(stream: &mut UnixStream) -> Result<[u8; N], Error> {
    let mut value = [0; N];
    stream.read_exact(&mut value).map_err(|_| Error::Io)?;
    Ok(value)
}

#[cfg(test)]
#[path = "display_tests.rs"]
mod tests;
