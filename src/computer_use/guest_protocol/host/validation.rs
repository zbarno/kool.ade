use base64::{Engine as _, engine::general_purpose::STANDARD as BASE64};
use image::{ImageFormat, ImageReader, Limits};
use sha2::{Digest, Sha256};
use std::io::Cursor;

use super::super::auth::GuestHandshakeCompleteV1;
use super::super::{GuestObservationV1, GuestProtocolErrorV1, frame::MAX_SCREENSHOT_PNG_BYTES};
use crate::computer_use::{ComputerActionV1, ComputerGuestReadinessV1, ComputerSessionV1};

pub(super) fn readiness_token(
    session: &ComputerSessionV1,
    completed: &GuestHandshakeCompleteV1,
) -> ComputerGuestReadinessV1 {
    ComputerGuestReadinessV1::new(
        session.identity.session_id.clone(),
        session.lease.generation,
        completed.boot_id.clone(),
        completed.transcript_sha256.clone(),
    )
}

pub(super) fn request_id_for_action(
    session: &ComputerSessionV1,
    action: &ComputerActionV1,
) -> String {
    let namespace =
        uuid::Uuid::parse_str(&session.identity.session_id).expect("validated session UUID");
    uuid::Uuid::new_v5(
        &namespace,
        format!(
            "koolade.computer-use.request.v1:{}:{}:{}",
            session.lease.generation, action.action_sequence, action.idempotency_key
        )
        .as_bytes(),
    )
    .to_string()
}

pub(super) fn validate_observation(
    observation: &GuestObservationV1,
    expected_session_id: &str,
    previous_revision: u64,
    previous_display_generation: u64,
) -> Result<GuestObservationV1, GuestProtocolErrorV1> {
    observation
        .validate_metadata()
        .map_err(|_| GuestProtocolErrorV1::Malformed)?;
    if observation.session_id != expected_session_id {
        return Err(GuestProtocolErrorV1::WrongSession);
    }
    if observation.observation_revision <= previous_revision
        || observation.display_generation < previous_display_generation
    {
        return Err(GuestProtocolErrorV1::StaleObservation);
    }
    if observation.screenshot_png_base64.len() > MAX_SCREENSHOT_PNG_BYTES * 4 / 3 + 8 {
        return Err(GuestProtocolErrorV1::Oversized);
    }
    let bytes = BASE64
        .decode(&observation.screenshot_png_base64)
        .map_err(|_| GuestProtocolErrorV1::Malformed)?;
    if bytes.is_empty() || bytes.len() > MAX_SCREENSHOT_PNG_BYTES {
        return Err(GuestProtocolErrorV1::Oversized);
    }
    let digest = sha256_hex(&bytes);
    if digest != observation.screenshot_sha256 || !bytes.starts_with(b"\x89PNG\r\n\x1a\n") {
        return Err(GuestProtocolErrorV1::AuthenticationFailed);
    }
    let mut reader = ImageReader::with_format(Cursor::new(&bytes), ImageFormat::Png);
    let mut limits = Limits::default();
    limits.max_image_width = Some(1920);
    limits.max_image_height = Some(1080);
    limits.max_alloc = Some(16 * 1024 * 1024);
    reader.limits(limits);
    let image = reader
        .decode()
        .map_err(|_| GuestProtocolErrorV1::Malformed)?;
    if image.width() != observation.width_px || image.height() != observation.height_px {
        return Err(GuestProtocolErrorV1::Malformed);
    }
    Ok(GuestObservationV1 {
        session_id: observation.session_id.clone(),
        frame_id: observation.frame_id.clone(),
        display_generation: observation.display_generation,
        observation_revision: observation.observation_revision,
        device_scale_numerator: observation.device_scale_numerator,
        device_scale_denominator: observation.device_scale_denominator,
        width_px: image.width(),
        height_px: image.height(),
        screenshot_png_base64: BASE64.encode(bytes),
        screenshot_sha256: digest,
        windows: observation.windows.clone(),
    })
}

pub(super) fn sha256_hex(bytes: &[u8]) -> String {
    Sha256::digest(bytes)
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect()
}
