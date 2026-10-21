use serde::{Deserialize, Serialize};

use super::{QaDriverActionV1, is_sha256, is_uuid, valid_text};
mod target;
pub use target::QaValidatedTargetV1;

/// App-owned observations made inside the pinned QA container. Missing GUI
/// components produce Unsupported; any host desktop/device exposure is a
/// policy violation and must never be downgraded to a warning.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct LinuxDesktopProbeV1 {
    pub schema_version: u16,
    pub qa_run_id: String,
    pub task_uid: String,
    pub image_digest: String,
    pub artifact_sha256: String,
    pub executable_sha256: String,
    pub display_width: u16,
    pub display_height: u16,
    pub dpi: u16,
    pub private_xvfb_ready: bool,
    pub llvmpipe_ready: bool,
    pub input_driver_ready: bool,
    pub screenshot_capture_ready: bool,
    pub isolated_home_ready: bool,
    pub process_scope_ready: bool,
    pub host_display_mounted: bool,
    pub host_wayland_mounted: bool,
    pub host_dbus_mounted: bool,
    pub host_xauthority_mounted: bool,
    pub host_input_mounted: bool,
    pub host_gpu_mounted: bool,
    pub host_audio_mounted: bool,
    pub evidence_refs: Vec<String>,
    pub recorded_at_unix_ms: u64,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(tag = "status", rename_all = "snake_case", deny_unknown_fields)]
pub enum LinuxDesktopCapabilityV1 {
    Ready {
        qa_run_id: String,
        task_uid: String,
        image_digest: String,
        artifact_sha256: String,
        display_width: u16,
        display_height: u16,
        evidence_refs: Vec<String>,
    },
    Unsupported {
        qa_run_id: String,
        task_uid: String,
        missing: Vec<String>,
        evidence_refs: Vec<String>,
    },
}

impl LinuxDesktopProbeV1 {
    pub fn assess(&self) -> Result<LinuxDesktopCapabilityV1, String> {
        if self.schema_version != 1
            || !is_uuid(&self.qa_run_id)
            || !is_uuid(&self.task_uid)
            || !is_sha256(&self.image_digest)
            || !is_sha256(&self.artifact_sha256)
            || !is_sha256(&self.executable_sha256)
            || self.artifact_sha256 != self.executable_sha256
            || !(1024..=7680).contains(&self.display_width)
            || !(768..=4320).contains(&self.display_height)
            || !(48..=960).contains(&self.dpi)
            || self.evidence_refs.len() < 3
            || self.evidence_refs.len() > 32
            || self.evidence_refs.iter().any(|item| {
                !valid_text(item, 1024)
                    || !item.starts_with(&format!("qa-evidence/{}/", self.qa_run_id))
                    || item.split('/').any(|part| part == ".." || part.is_empty())
            })
            || self.recorded_at_unix_ms == 0
        {
            return Err(
                "Linux desktop capability probe is malformed or not bound to the built artifact"
                    .into(),
            );
        }
        if self.host_display_mounted
            || self.host_wayland_mounted
            || self.host_dbus_mounted
            || self.host_xauthority_mounted
            || self.host_input_mounted
            || self.host_gpu_mounted
            || self.host_audio_mounted
        {
            return Err(
                "Linux desktop QA cannot expose host display, devices, or user services".into(),
            );
        }
        let mut missing = Vec::new();
        let image_lock = crate::execution::docker::images::ImageLock::bundled()
            .map_err(|failure| failure.to_string())?;
        if let Err(failure) = image_lock.select(
            crate::execution::docker::images::RuntimeProfile::BrowserQa,
            "amd64",
        ) {
            missing.push(format!(
                "approved Linux desktop QA image: {}",
                failure.message
            ));
        }
        for (ready, name) in [
            (self.private_xvfb_ready, "private Xvfb display"),
            (self.llvmpipe_ready, "Mesa llvmpipe software renderer"),
            (self.input_driver_ready, "bounded mouse and keyboard driver"),
            (self.screenshot_capture_ready, "private screenshot capture"),
            (self.isolated_home_ready, "per-run HOME and XDG profile"),
            (self.process_scope_ready, "per-run process ownership"),
        ] {
            if !ready {
                missing.push(name.to_owned());
            }
        }
        if !missing.is_empty() {
            return Ok(LinuxDesktopCapabilityV1::Unsupported {
                qa_run_id: self.qa_run_id.clone(),
                task_uid: self.task_uid.clone(),
                missing,
                evidence_refs: self.evidence_refs.clone(),
            });
        }
        Ok(LinuxDesktopCapabilityV1::Ready {
            qa_run_id: self.qa_run_id.clone(),
            task_uid: self.task_uid.clone(),
            image_digest: self.image_digest.clone(),
            artifact_sha256: self.artifact_sha256.clone(),
            display_width: self.display_width,
            display_height: self.display_height,
            evidence_refs: self.evidence_refs.clone(),
        })
    }
}

/// Current-run window inventory is the authority for desktop window IDs.
/// Model-provided IDs, browser navigation, and off-screen geometry are denied.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct LinuxDesktopSessionV1 {
    pub qa_run_id: String,
    pub display_width: u16,
    pub display_height: u16,
    pub windows: Vec<LinuxDesktopWindowV1>,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct LinuxDesktopWindowV1 {
    pub window_id: String,
    pub x: u16,
    pub y: u16,
    pub width: u16,
    pub height: u16,
}

impl LinuxDesktopSessionV1 {
    pub fn validate_action(&self, action: &QaDriverActionV1) -> Result<(), String> {
        if !is_uuid(&self.qa_run_id)
            || !(1024..=7680).contains(&self.display_width)
            || !(768..=4320).contains(&self.display_height)
            || self.windows.len() > 128
            || self.windows.iter().any(|window| {
                !valid_text(&window.window_id, 128)
                    || window.width == 0
                    || window.height == 0
                    || window.x.saturating_add(window.width) > self.display_width
                    || window.y.saturating_add(window.height) > self.display_height
            })
            || self.windows.iter().enumerate().any(|(index, window)| {
                self.windows[..index]
                    .iter()
                    .any(|prior| prior.window_id == window.window_id)
            })
        {
            return Err("Linux desktop session is invalid".into());
        }
        let window = |id: &str| self.windows.iter().find(|window| window.window_id == id);
        let inside_window = |id: &str, x: u16, y: u16| {
            window(id).is_some_and(|target| {
                x >= target.x
                    && y >= target.y
                    && x < target.x.saturating_add(target.width)
                    && y < target.y.saturating_add(target.height)
            })
        };
        let allowed = match action {
            QaDriverActionV1::LaunchValidatedTarget
            | QaDriverActionV1::WaitForWindow
            | QaDriverActionV1::ListWindows
            | QaDriverActionV1::Screenshot
            | QaDriverActionV1::ReadAppLogs => true,
            QaDriverActionV1::FocusWindow { window_id }
            | QaDriverActionV1::CloseWindow { window_id } => window(window_id).is_some(),
            QaDriverActionV1::AssertRegionVisible {
                window_id,
                x,
                y,
                width,
                height,
            } => {
                window(window_id).is_some()
                    && *width >= 4
                    && *height >= 4
                    && *width <= 256
                    && *height <= 256
                    && x.saturating_add(*width) <= window(window_id).map_or(0, |item| item.width)
                    && y.saturating_add(*height) <= window(window_id).map_or(0, |item| item.height)
            }
            QaDriverActionV1::ResizeWindow {
                window_id,
                width,
                height,
            } => {
                window(window_id).is_some()
                    && (320..=self.display_width).contains(width)
                    && (240..=self.display_height).contains(height)
            }
            QaDriverActionV1::Click { window_id, x, y }
            | QaDriverActionV1::DoubleClick { window_id, x, y } => inside_window(window_id, *x, *y),
            QaDriverActionV1::Drag {
                window_id,
                start_x,
                start_y,
                end_x,
                end_y,
            } => {
                inside_window(window_id, *start_x, *start_y)
                    && inside_window(window_id, *end_x, *end_y)
            }
            QaDriverActionV1::Scroll { window_id, .. } => window(window_id).is_some(),
            QaDriverActionV1::TypeText { text } => text.len() <= 16 * 1024 && !text.contains('\0'),
            QaDriverActionV1::KeyChord { keys } => {
                !keys.is_empty() && keys.len() <= 8 && keys.iter().all(|key| valid_text(key, 32))
            }
            QaDriverActionV1::WaitForVisible { .. }
            | QaDriverActionV1::NavigateAllowedUrl { .. }
            | QaDriverActionV1::BrowserOpen { .. }
            | QaDriverActionV1::BrowserClick { .. }
            | QaDriverActionV1::BrowserFill { .. }
            | QaDriverActionV1::BrowserPress { .. }
            | QaDriverActionV1::BrowserSelect { .. }
            | QaDriverActionV1::BrowserWaitForVisible { .. }
            | QaDriverActionV1::BrowserAssertText { .. }
            | QaDriverActionV1::BrowserAssertUrl { .. }
            | QaDriverActionV1::BrowserSetViewport { .. }
            | QaDriverActionV1::BrowserScreenshot
            | QaDriverActionV1::BrowserScroll { .. }
            | QaDriverActionV1::FindByRole { .. }
            | QaDriverActionV1::FindByTestId { .. }
            | QaDriverActionV1::Fill { .. }
            | QaDriverActionV1::Press { .. }
            | QaDriverActionV1::Select { .. }
            | QaDriverActionV1::CollectConsole
            | QaDriverActionV1::CollectNetworkFailures
            | QaDriverActionV1::ApiRequest { .. } => false,
        };
        if allowed {
            Ok(())
        } else {
            Err(
                "Linux desktop action is outside this QA session's window or capability bounds"
                    .into(),
            )
        }
    }
}
