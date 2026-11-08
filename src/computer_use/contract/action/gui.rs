use serde::{Deserialize, Serialize};

/// Geometry copied from one authenticated display observation. Pointer input
/// is rejected if its frame, display generation, window revision, or bounds
/// differ from the latest controller-accepted observation.
#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct ComputerFrameReferenceV1 {
    pub frame_id: String,
    pub display_generation: u64,
    pub observation_revision: u64,
    pub display_width_px: u32,
    pub display_height_px: u32,
    pub device_scale_numerator: u32,
    pub device_scale_denominator: u32,
    pub window_id: String,
    pub window_generation: u64,
    pub window_bounds: ComputerWindowBoundsV1,
}

impl ComputerFrameReferenceV1 {
    pub(super) fn validate_point(&self, x: u32, y: u32) -> Result<(), ()> {
        let bounds = &self.window_bounds;
        let right = i64::from(bounds.left) + i64::from(bounds.width_px);
        let bottom = i64::from(bounds.top) + i64::from(bounds.height_px);
        if !is_uuid(&self.frame_id)
            || !is_uuid(&self.window_id)
            || self.display_generation == 0
            || self.observation_revision == 0
            || self.display_width_px == 0
            || self.display_height_px == 0
            || self.display_width_px > 16_384
            || self.display_height_px > 16_384
            || self.device_scale_numerator == 0
            || self.device_scale_denominator == 0
            || self.device_scale_numerator > 16
            || self.device_scale_denominator > 16
            || self.window_generation == 0
            || bounds.width_px == 0
            || bounds.height_px == 0
            || bounds.width_px > 16_384
            || bounds.height_px > 16_384
            || x >= self.display_width_px
            || y >= self.display_height_px
            || !(i64::from(bounds.left)..right).contains(&i64::from(x))
            || !(i64::from(bounds.top)..bottom).contains(&i64::from(y))
        {
            return Err(());
        }
        Ok(())
    }
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct ComputerWindowBoundsV1 {
    pub left: i32,
    pub top: i32,
    pub width_px: u32,
    pub height_px: u32,
}

#[derive(Clone, Copy, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(tag = "kind", content = "value", rename_all = "snake_case")]
pub enum ComputerKeyV1 {
    Named(super::KeyCodeV1),
    Character(char),
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum ComputerWaitConditionV1 {
    ScreenDiffers { screenshot_sha256: String },
    WindowTitleContains { text: String },
    WindowVisible { window_id: String },
    WindowClosed { window_id: String },
}

impl ComputerWaitConditionV1 {
    pub(super) fn validate(&self) -> Result<(), ()> {
        match self {
            Self::ScreenDiffers { screenshot_sha256 }
                if screenshot_sha256.len() != 64
                    || !screenshot_sha256
                        .bytes()
                        .all(|byte| byte.is_ascii_hexdigit()) =>
            {
                Err(())
            }
            Self::WindowTitleContains { text }
                if text.is_empty() || text.len() > 128 || text.chars().any(char::is_control) =>
            {
                Err(())
            }
            Self::WindowVisible { window_id } | Self::WindowClosed { window_id }
                if !is_uuid(window_id) =>
            {
                Err(())
            }
            _ => Ok(()),
        }
    }
}

fn is_uuid(value: &str) -> bool {
    uuid::Uuid::parse_str(value).is_ok_and(|id| id.to_string() == value)
}
