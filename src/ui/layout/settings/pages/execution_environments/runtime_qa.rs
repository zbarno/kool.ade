use crate::{execution::docker::images::ImageLock, ui::theme};

static IMAGE_LOCK: std::sync::OnceLock<Option<ImageLock>> = std::sync::OnceLock::new();

pub(super) fn paint(ui: &mut egui::Ui) {
    ui.label(theme::section_heading("Runtime QA"));
    ui.colored_label(theme::WARNING, "Unavailable — release gates pending (#171)");
    ui.label(theme::helper_text(
        "Docker readiness does not mean browser or desktop QA is ready. QA execution stays disabled until pinned images, isolated lifecycle, and release security checks are approved.",
    ));

    let lock = IMAGE_LOCK
        .get_or_init(|| ImageLock::bundled().ok())
        .as_ref();
    ui.label(format!(
        "Browser QA image: {}",
        profile_status(lock, "browser_qa")
    ));
    ui.label(format!(
        "Linux desktop QA image and software display: {}",
        profile_status(lock, "desktop_qa")
    ));
    ui.label("Preview port relay: not verified for release (#171).");
    ui.label("Evidence capacity and retention: not reported as runtime readiness yet.");
    ui.label("Selected CLI QA-role compatibility: not evaluated here; see Coding Tools.");
    ui.label(theme::metadata_text(
        "Complete the #171 image, lifecycle, security, and CI gates before enabling or advertising Runtime QA.",
    ));
}

fn profile_status(lock: Option<&ImageLock>, name: &str) -> String {
    let Some(lock) = lock else {
        return "unavailable; bundled image lock could not be validated".into();
    };
    let Some(variants) = lock.profiles.get(name) else {
        return "unavailable; no approved pinned image is present".into();
    };
    if variants.is_empty() {
        return "unavailable; no approved pinned image is present".into();
    }
    if variants.iter().all(|variant| !variant.runtime_enabled) {
        return "unavailable; pinned image is not runtime-enabled by policy".into();
    }
    "pinned profile is enabled by image policy; runtime QA remains unavailable pending release gates (#171)".into()
}

#[cfg(test)]
mod tests {
    use super::profile_status;
    use crate::execution::docker::images::{ImageLock, ImageVariant};
    use std::collections::BTreeMap;

    fn lock_with_profile(variants: Vec<ImageVariant>) -> ImageLock {
        ImageLock {
            schema_version: 1,
            image_set_version: "synthetic".into(),
            profiles: BTreeMap::from([("browser_qa".into(), variants)]),
        }
    }

    fn variant(runtime_enabled: bool) -> ImageVariant {
        ImageVariant {
            platform: "linux/amd64".into(),
            reference: "ghcr.io/example/browser@sha256:synthetic".into(),
            digest: "sha256:synthetic".into(),
            tool_versions: BTreeMap::new(),
            runtime_enabled,
            build_recipe_sha256: Some("sha256:synthetic".into()),
        }
    }

    #[test]
    fn enabled_image_policy_does_not_claim_runtime_qa_is_released() {
        let lock = lock_with_profile(vec![variant(true)]);

        let status = profile_status(Some(&lock), "browser_qa");

        assert!(status.contains("enabled by image policy"));
        assert!(status.contains("remains unavailable pending release gates (#171)"));
    }

    #[test]
    fn missing_empty_and_disabled_profiles_remain_unavailable() {
        let empty_lock = ImageLock {
            schema_version: 1,
            image_set_version: "synthetic".into(),
            profiles: BTreeMap::new(),
        };
        let empty_profile = lock_with_profile(Vec::new());
        let disabled_profile = lock_with_profile(vec![variant(false)]);

        for status in [
            profile_status(None, "browser_qa"),
            profile_status(Some(&empty_lock), "browser_qa"),
            profile_status(Some(&empty_profile), "browser_qa"),
            profile_status(Some(&disabled_profile), "browser_qa"),
        ] {
            assert!(
                status.starts_with("unavailable;"),
                "unexpected status: {status}"
            );
        }
    }
}
