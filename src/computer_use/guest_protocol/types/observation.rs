// The host observation validator is compiled behind the Linux guest transport.
#![allow(dead_code)]

use crate::computer_use::ComputerWindowBoundsV1;
use serde::{Deserialize, Serialize};

/// Authenticated frame metadata. `observation_revision` advances for every
/// frame in a boot session; `display_generation` advances whenever display
/// geometry changes. PNG bytes are bounded and hashed separately.
#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct GuestObservationV1 {
    pub session_id: String,
    pub frame_id: String,
    pub display_generation: u64,
    pub observation_revision: u64,
    pub device_scale_numerator: u32,
    pub device_scale_denominator: u32,
    pub width_px: u32,
    pub height_px: u32,
    pub screenshot_png_base64: String,
    pub screenshot_sha256: String,
    pub windows: Vec<GuestWindowObservationV1>,
}

impl GuestObservationV1 {
    pub(crate) fn validate_metadata(&self) -> Result<(), &'static str> {
        if !is_uuid(&self.session_id)
            || !is_uuid(&self.frame_id)
            || self.display_generation == 0
            || self.observation_revision == 0
            || self.device_scale_numerator == 0
            || self.device_scale_denominator == 0
            || self.device_scale_numerator > 16
            || self.device_scale_denominator > 16
            || !(1..=16_384).contains(&self.width_px)
            || !(1..=16_384).contains(&self.height_px)
            || !valid_digest(&self.screenshot_sha256)
            || self.windows.len() > 32
            || self.windows.iter().any(|window| !window.validate())
        {
            return Err("guest observation metadata is invalid");
        }
        Ok(())
    }

    pub(crate) fn matches_frame_reference(
        &self,
        target: &crate::computer_use::ComputerFrameReferenceV1,
    ) -> bool {
        self.frame_id == target.frame_id
            && self.display_generation == target.display_generation
            && self.observation_revision == target.observation_revision
            && self.width_px == target.display_width_px
            && self.height_px == target.display_height_px
            && self.device_scale_numerator == target.device_scale_numerator
            && self.device_scale_denominator == target.device_scale_denominator
            && self.windows.iter().any(|window| {
                window.window_id == target.window_id
                    && window.generation == target.window_generation
                    && window.bounds == target.window_bounds
            })
    }
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct GuestWindowObservationV1 {
    pub window_id: String,
    pub generation: u64,
    pub bounds: ComputerWindowBoundsV1,
    pub title: String,
    pub app_id: Option<String>,
    pub focused: bool,
}

impl GuestWindowObservationV1 {
    fn validate(&self) -> bool {
        let b = &self.bounds;
        is_uuid(&self.window_id)
            && self.generation > 0
            && (1..=16_384).contains(&b.width_px)
            && (1..=16_384).contains(&b.height_px)
            && self.title.len() <= 256
            && !self.title.chars().any(char::is_control)
            && self.app_id.as_ref().is_none_or(|id| valid_app_id(id))
    }
}

fn is_uuid(value: &str) -> bool {
    uuid::Uuid::parse_str(value).is_ok_and(|id| id.to_string() == value)
}

fn valid_digest(value: &str) -> bool {
    value.len() == 64 && value.bytes().all(|b| b.is_ascii_hexdigit())
}

fn valid_app_id(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 64
        && value
            .bytes()
            .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-' || b == b'.')
}
