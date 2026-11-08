use super::validation::sha256_hex;
use super::*;
use crate::computer_use::guest_protocol::{
    GuestObservationV1, GuestProtocolErrorV1, MAX_SCREENSHOT_PNG_BYTES,
};
use base64::Engine as _;
use base64::engine::general_purpose::STANDARD as BASE64;
use image::{ColorType, ImageEncoder, codecs::png::PngEncoder};

/// Production boundary: the controller decodes guest PNG data with bounded
/// dimensions and allocation before it accepts an observation. A serde-only
/// unit test cannot prove the image decoder's decompression bounds. This uses
/// in-memory bytes only and has negligible setup cost; it requires no KVM or
/// display server.
#[test]
fn screenshot_validation_rejects_wrong_digest_dimensions_and_oversize() {
    let png = tiny_png();
    let digest = sha256_hex(&png);
    let valid = GuestObservationV1 {
        session_id: "00000000-0000-4000-8000-000000000001".into(),
        frame_id: "00000000-0000-4000-8000-000000000002".into(),
        display_generation: 1,
        observation_revision: 1,
        device_scale_numerator: 1,
        device_scale_denominator: 1,
        width_px: 2,
        height_px: 1,
        screenshot_png_base64: BASE64.encode(&png),
        screenshot_sha256: digest,
        windows: vec![],
    };
    let session_id = valid.session_id.clone();
    let validate = |observation| validate_observation(observation, &session_id, 0, 0);
    let accepted = validate(&valid).unwrap();
    assert_eq!(accepted.width_px, 2);
    assert_eq!(accepted.height_px, 1);

    let mut mismatch = valid.clone();
    mismatch.screenshot_sha256 = sha256_hex(b"different image");
    assert_eq!(
        validate(&mismatch),
        Err(GuestProtocolErrorV1::AuthenticationFailed)
    );
    let mut wrong_dimensions = valid.clone();
    wrong_dimensions.width_px = 1;
    assert_eq!(
        validate(&wrong_dimensions),
        Err(GuestProtocolErrorV1::Malformed)
    );
    let too_large = GuestObservationV1 {
        screenshot_png_base64: "A".repeat(MAX_SCREENSHOT_PNG_BYTES * 2),
        ..valid.clone()
    };
    assert_eq!(validate(&too_large), Err(GuestProtocolErrorV1::Oversized));
}

fn tiny_png() -> Vec<u8> {
    let pixels = [0_u8, 0, 0, 255, 255, 255];
    let mut bytes = Vec::new();
    PngEncoder::new(&mut bytes)
        .write_image(&pixels, 2, 1, ColorType::Rgb8.into())
        .unwrap();
    bytes
}
