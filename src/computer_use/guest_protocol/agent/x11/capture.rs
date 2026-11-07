use crate::computer_use::guest_protocol::{
    GuestObservationV1, GuestProtocolErrorV1, frame::MAX_SCREENSHOT_PNG_BYTES,
};
use base64::{Engine as _, engine::general_purpose::STANDARD as BASE64};
use image::{ColorType, ImageEncoder, codecs::png::PngEncoder};
use sha2::{Digest, Sha256};
use x11rb::{
    connection::Connection,
    protocol::xproto::{self, ConnectionExt as _, ImageFormat},
};

const MAX_WIDTH: u16 = 1920;
const MAX_HEIGHT: u16 = 1080;

pub(in crate::computer_use::guest_protocol::agent) fn capture_png()
-> Result<GuestObservationV1, GuestProtocolErrorV1> {
    let (connection, screen_index) = super::input::connect()?;
    let screen = &connection.setup().roots[screen_index];
    let geometry = connection
        .get_geometry(screen.root)
        .map_err(|_| GuestProtocolErrorV1::Io)?
        .reply()
        .map_err(|_| GuestProtocolErrorV1::Io)?;
    if geometry.width == 0
        || geometry.height == 0
        || geometry.width > MAX_WIDTH
        || geometry.height > MAX_HEIGHT
    {
        return Err(GuestProtocolErrorV1::Oversized);
    }
    let image = connection
        .get_image(
            ImageFormat::Z_PIXMAP,
            screen.root,
            0,
            0,
            geometry.width,
            geometry.height,
            u32::MAX,
        )
        .map_err(|_| GuestProtocolErrorV1::Io)?
        .reply()
        .map_err(|_| GuestProtocolErrorV1::Io)?;
    let format = connection
        .setup()
        .pixmap_formats
        .iter()
        .find(|format| format.depth == image.depth)
        .ok_or(GuestProtocolErrorV1::Malformed)?;
    let visual = screen
        .allowed_depths
        .iter()
        .filter(|depth| depth.depth == image.depth)
        .flat_map(|depth| depth.visuals.iter())
        .find(|visual| visual.visual_id == screen.root_visual)
        .ok_or(GuestProtocolErrorV1::Malformed)?;
    let rgb = unpack_rgb(
        &image.data,
        geometry.width,
        geometry.height,
        PixelFormat {
            bits_per_pixel: format.bits_per_pixel,
            scanline_pad: format.scanline_pad,
            byte_order: connection.setup().image_byte_order,
            red_mask: visual.red_mask,
            green_mask: visual.green_mask,
            blue_mask: visual.blue_mask,
        },
    )?;
    let mut png = Vec::new();
    PngEncoder::new(&mut png)
        .write_image(
            &rgb,
            geometry.width as u32,
            geometry.height as u32,
            ColorType::Rgb8.into(),
        )
        .map_err(|_| GuestProtocolErrorV1::Malformed)?;
    if png.len() > MAX_SCREENSHOT_PNG_BYTES {
        return Err(GuestProtocolErrorV1::Oversized);
    }
    let digest = Sha256::digest(&png)
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect();
    Ok(GuestObservationV1 {
        width_px: geometry.width as u32,
        height_px: geometry.height as u32,
        screenshot_png_base64: BASE64.encode(png),
        screenshot_sha256: digest,
    })
}

struct PixelFormat {
    bits_per_pixel: u8,
    scanline_pad: u8,
    byte_order: xproto::ImageOrder,
    red_mask: u32,
    green_mask: u32,
    blue_mask: u32,
}

fn unpack_rgb(
    data: &[u8],
    width: u16,
    height: u16,
    format: PixelFormat,
) -> Result<Vec<u8>, GuestProtocolErrorV1> {
    if !matches!(format.bits_per_pixel, 16 | 24 | 32)
        || format.scanline_pad == 0
        || !format.scanline_pad.is_multiple_of(8)
    {
        return Err(GuestProtocolErrorV1::UnsupportedCapability);
    }
    let pixel_bytes = format.bits_per_pixel as usize / 8;
    let row_bits = width as usize * format.bits_per_pixel as usize;
    let row_pad_bits = format.scanline_pad as usize;
    let stride = row_bits.div_ceil(row_pad_bits) * (row_pad_bits / 8);
    let required = stride
        .checked_mul(height as usize)
        .filter(|required| *required <= data.len())
        .ok_or(GuestProtocolErrorV1::Malformed)?;
    if required > 16 * 1024 * 1024 {
        return Err(GuestProtocolErrorV1::Oversized);
    }
    let mut rgb = vec![0; width as usize * height as usize * 3];
    for y in 0..height as usize {
        for x in 0..width as usize {
            let offset = y * stride + x * pixel_bytes;
            let pixel = match format.byte_order {
                xproto::ImageOrder::LSB_FIRST => {
                    let mut raw = [0; 4];
                    raw[..pixel_bytes].copy_from_slice(&data[offset..offset + pixel_bytes]);
                    u32::from_le_bytes(raw)
                }
                xproto::ImageOrder::MSB_FIRST => {
                    let mut raw = [0; 4];
                    raw[4 - pixel_bytes..].copy_from_slice(&data[offset..offset + pixel_bytes]);
                    u32::from_be_bytes(raw)
                }
                _ => return Err(GuestProtocolErrorV1::UnsupportedCapability),
            };
            let target = (y * width as usize + x) * 3;
            rgb[target] = channel(pixel, format.red_mask);
            rgb[target + 1] = channel(pixel, format.green_mask);
            rgb[target + 2] = channel(pixel, format.blue_mask);
        }
    }
    Ok(rgb)
}

fn channel(pixel: u32, mask: u32) -> u8 {
    if mask == 0 {
        return 0;
    }
    let shift = mask.trailing_zeros();
    let max = mask >> shift;
    ((((pixel & mask) >> shift) as u64) * 255 / max as u64) as u8
}
