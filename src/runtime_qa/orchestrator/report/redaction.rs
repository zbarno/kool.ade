use image::{ImageEncoder, Rgba};
use sha2::Digest;

use crate::runtime_qa::{QaDriverScreenshotV1, QaRedactionRegionV1};

#[derive(Debug)]
pub(super) struct RedactedScreenshot {
    pub bytes: Vec<u8>,
    pub sha256: String,
    pub mask_descriptions: Vec<String>,
}

pub(super) fn apply(
    screenshot: &QaDriverScreenshotV1,
    regions: &[QaRedactionRegionV1],
    max_bytes: u64,
    display_width: u16,
    display_height: u16,
) -> Result<RedactedScreenshot, String> {
    validate_png_header(screenshot, max_bytes, display_width, display_height)?;
    let input_hash = format!("{:x}", sha2::Sha256::digest(&screenshot.bytes));
    if screenshot.sha256 != input_hash {
        return Err("QA driver screenshot hash does not match its source bytes".into());
    }
    let decoded = image::load_from_memory(&screenshot.bytes)
        .map_err(|_| "QA driver screenshot is not a decodable image")?;
    if decoded.width() != u32::from(screenshot.width)
        || decoded.height() != u32::from(screenshot.height)
    {
        return Err("QA driver screenshot dimensions do not match its image bytes".into());
    }
    let mask_descriptions = regions
        .iter()
        .map(|region| {
            format!(
                "rect:{},{},{},{}",
                region.x, region.y, region.width, region.height
            )
        })
        .collect::<Vec<_>>();
    if regions.is_empty() {
        return Ok(RedactedScreenshot {
            bytes: screenshot.bytes.clone(),
            sha256: input_hash,
            mask_descriptions,
        });
    }
    let mut pixels = decoded.to_rgba8();
    for region in regions {
        let x_end = u32::from(region.x) + u32::from(region.width);
        let y_end = u32::from(region.y) + u32::from(region.height);
        if region.width == 0
            || region.height == 0
            || x_end > pixels.width()
            || y_end > pixels.height()
        {
            return Err(
                "approved screenshot redaction region is outside the captured image".into(),
            );
        }
        for y in u32::from(region.y)..y_end {
            for x in u32::from(region.x)..x_end {
                pixels.put_pixel(x, y, Rgba([0, 0, 0, 255]));
            }
        }
    }
    let mut bytes = Vec::new();
    image::codecs::png::PngEncoder::new(&mut bytes)
        .write_image(
            pixels.as_raw(),
            pixels.width(),
            pixels.height(),
            image::ExtendedColorType::Rgba8,
        )
        .map_err(|error| format!("could not encode redacted QA screenshot: {error}"))?;
    let sha256 = format!("{:x}", sha2::Sha256::digest(&bytes));
    Ok(RedactedScreenshot {
        bytes,
        sha256,
        mask_descriptions,
    })
}

fn validate_png_header(
    screenshot: &QaDriverScreenshotV1,
    max_bytes: u64,
    display_width: u16,
    display_height: u16,
) -> Result<(), String> {
    const PNG_SIGNATURE: &[u8; 8] = b"\x89PNG\r\n\x1a\n";
    if screenshot.bytes.len() < 24
        || screenshot.bytes.get(..8) != Some(PNG_SIGNATURE.as_slice())
        || screenshot.bytes.get(12..16) != Some(b"IHDR")
    {
        return Err("QA driver screenshot is missing a valid PNG header".into());
    }
    if screenshot.bytes.len() as u64 > max_bytes {
        return Err("QA driver screenshot exceeds its evidence byte bound".into());
    }
    let width = u32::from_be_bytes(screenshot.bytes[16..20].try_into().unwrap());
    let height = u32::from_be_bytes(screenshot.bytes[20..24].try_into().unwrap());
    if width != u32::from(screenshot.width)
        || height != u32::from(screenshot.height)
        || width > u32::from(display_width.min(7680))
        || height > u32::from(display_height.min(4320))
        || u64::from(width).saturating_mul(u64::from(height)) > 33_177_600
    {
        return Err(
            "QA driver screenshot dimensions exceed or disagree with their metadata".into(),
        );
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::runtime_qa::QaDriverScreenshotV1;

    fn screenshot() -> QaDriverScreenshotV1 {
        let image = image::RgbaImage::from_pixel(4, 4, Rgba([240, 120, 60, 255]));
        let mut bytes = Vec::new();
        image::codecs::png::PngEncoder::new(&mut bytes)
            .write_image(image.as_raw(), 4, 4, image::ExtendedColorType::Rgba8)
            .unwrap();
        QaDriverScreenshotV1 {
            id: "33333333-3333-4333-8333-333333333333".into(),
            scenario_id: "22222222-2222-4222-8222-222222222222".into(),
            sha256: format!("{:x}", sha2::Sha256::digest(&bytes)),
            width: 4,
            height: 4,
            source_action: 0,
            redaction_masks: vec![],
            bytes,
        }
    }

    #[test]
    fn masks_pixels_before_hashing_persisted_evidence() {
        let source = screenshot();
        let result = apply(
            &source,
            &[QaRedactionRegionV1 {
                x: 1,
                y: 1,
                width: 2,
                height: 2,
            }],
            1024,
            16,
            12,
        )
        .unwrap();
        assert_ne!(result.sha256, source.sha256);
        assert_eq!(result.mask_descriptions, ["rect:1,1,2,2"]);
        let image = image::load_from_memory(&result.bytes).unwrap().to_rgba8();
        assert_eq!(image.get_pixel(1, 1), &Rgba([0, 0, 0, 255]));
        assert_eq!(image.get_pixel(2, 2), &Rgba([0, 0, 0, 255]));
        assert_eq!(image.get_pixel(0, 0), &Rgba([240, 120, 60, 255]));
    }

    #[test]
    fn rejects_redaction_regions_outside_screenshot() {
        let source = screenshot();
        assert!(
            apply(
                &source,
                &[QaRedactionRegionV1 {
                    x: 3,
                    y: 3,
                    width: 2,
                    height: 1,
                }],
                1024,
                16,
                12,
            )
            .unwrap_err()
            .contains("outside the captured image")
        );
    }

    #[test]
    fn rejects_oversized_png_dimensions_before_image_decode() {
        let mut screenshot = screenshot();
        screenshot.bytes[16..20].copy_from_slice(&65_535_u32.to_be_bytes());
        screenshot.bytes[20..24].copy_from_slice(&65_535_u32.to_be_bytes());
        screenshot.width = u16::MAX;
        screenshot.height = u16::MAX;
        screenshot.sha256 = format!("{:x}", sha2::Sha256::digest(&screenshot.bytes));

        assert!(
            apply(&screenshot, &[], 1024, u16::MAX, u16::MAX)
                .unwrap_err()
                .contains("dimensions exceed or disagree")
        );
    }

    #[test]
    fn rejects_dimensions_above_the_driver_viewport_before_decode() {
        let mut screenshot = screenshot();
        screenshot.bytes[16..20].copy_from_slice(&65_535_u32.to_be_bytes());
        screenshot.bytes[20..24].copy_from_slice(&506_u32.to_be_bytes());
        screenshot.width = u16::MAX;
        screenshot.height = 506;
        screenshot.sha256 = format!("{:x}", sha2::Sha256::digest(&screenshot.bytes));

        assert!(
            apply(&screenshot, &[], 1024, 768, 768)
                .unwrap_err()
                .contains("dimensions exceed or disagree")
        );
    }
}
