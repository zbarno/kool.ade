mod confirmation;
use confirmation::{Confirmation, confirmation, set_confirmation_pending};

use crate::{
    execution::{
        contract::HarnessRole,
        docker::{
            environment::DockerEnvironmentStatus,
            images::{ImageLock, RuntimeProfile},
        },
    },
    persistence::{
        experimental_features::{self, ExperimentalFeature, ExperimentalFeatureSettingsV1},
        harness_settings::{self, HarnessSettings},
    },
    ui::theme,
};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum CapabilityState {
    Ready,
    ExperimentalReady,
    Blocked,
    NotImplemented,
    Unsupported,
    ReleaseEnabled,
}

impl CapabilityState {
    fn label(self) -> &'static str {
        match self {
            Self::Ready => "Ready",
            Self::ExperimentalReady => "Experimental ready",
            Self::Blocked => "Blocked by prerequisite",
            Self::NotImplemented => "Not implemented",
            Self::Unsupported => "Unsupported platform",
            Self::ReleaseEnabled => "Release enabled",
        }
    }
}

pub(super) fn paint(ui: &mut egui::Ui, docker: &DockerEnvironmentStatus, lock: Option<&ImageLock>) {
    ui.label(theme::section_heading("Experimental Features"));
    ui.label(theme::helper_text("Experimental capabilities are local to this device, opt in explicitly, and remain separate from release approval."));
    let (mut settings, load_error) = experimental_features::load();
    if let Some(error) = load_error {
        ui.colored_label(theme::WARNING, error);
    }

    let mut enable = settings.enabled;
    if ui
        .checkbox(&mut enable, "Enable experimental features on this machine")
        .changed()
    {
        if enable {
            set_confirmation_pending(ui.ctx(), Confirmation::Mode);
        } else {
            settings.enabled = false;
            save(ui, &settings);
        }
    }
    if settings.enabled {
        ui.colored_label(theme::WARNING, "Experimental mode is ON. These capabilities are unverified and do not satisfy release gates. Turn this off to prevent new experimental QA dispatch.");
        if ui.button("Turn experimental mode off").clicked() {
            settings.enabled = false;
            save(ui, &settings);
        }
    } else {
        ui.label(theme::metadata_text("Experimental mode is OFF. Capabilities and historical evidence remain visible; new experimental runs are disabled."));
    }

    let harness = harness_settings::load().0;
    let reviewer = reviewer_ready(&harness);
    let docker_state = if docker.engine_ready && docker.rootless == Some(true) {
        CapabilityState::Ready
    } else {
        CapabilityState::Blocked
    };
    ui.horizontal_wrapped(|ui| {
        ui.label(egui::RichText::new("Rootless Docker execution").strong());
        ui.label(docker_state.label());
        ui.label(theme::helper_text(if docker_state == CapabilityState::Ready {
            "Ready for existing isolated containers; this does not enable QA or imply release approval."
        } else {
            "Requires the app's existing rootless Docker readiness checks. Rootful or host execution is never a fallback."
        }));
    });
    let registry = [
        (
            "Browser Runtime QA",
            Some(ExperimentalFeature::BrowserQa),
            RuntimeProfile::BrowserQa,
            "Supported on Linux amd64 only.",
        ),
        (
            "Linux desktop Runtime QA",
            Some(ExperimentalFeature::LinuxDesktopQa),
            RuntimeProfile::DesktopQa,
            "Supported on Linux amd64 only.",
        ),
    ];
    ui.add_space(theme::spacing::S);
    for (name, feature, profile, platform) in registry {
        let state = capability_state(feature.unwrap(), profile, docker, lock, &settings, reviewer);
        ui.horizontal_wrapped(|ui| {
            ui.label(egui::RichText::new(name).strong());
            ui.label(state.label());
            if let Some(feature) = feature {
                let mut enabled = settings.enabled_features.contains(&feature);
                let response = ui.add_enabled(
                    settings.enabled
                        && !matches!(
                            state,
                            CapabilityState::Unsupported | CapabilityState::ReleaseEnabled
                        ),
                    egui::Checkbox::new(&mut enabled, "Enable"),
                );
                if response.changed() {
                    if enabled {
                        settings.enabled_features.insert(feature);
                    } else {
                        settings.enabled_features.remove(&feature);
                    }
                    save(ui, &settings);
                }
            }
        });
        ui.label(theme::helper_text(blocker(
            state,
            feature.unwrap(),
            profile,
            docker,
            lock,
            &settings,
            reviewer,
            platform,
        )));
    }
    ui.horizontal_wrapped(|ui| {
        ui.label(egui::RichText::new("VM Computer Use").strong());
        ui.label(CapabilityState::NotImplemented.label());
        ui.label(theme::helper_text("Implementation is tracked by #375 and #389; there is no switch or host desktop fallback."));
    });

    if ui
        .button("Enable all testable experimental features")
        .clicked()
    {
        set_confirmation_pending(ui.ctx(), Confirmation::AllTestable);
    }
    confirmation(ui, &mut settings, docker, lock, reviewer);
    ui.label(theme::metadata_text(format!(
        "QA reviewer route: {}",
        if reviewer {
            "fresh reviewer capability"
        } else {
            "blocked; select and probe a QA reviewer in Coding Tools"
        }
    )));
    ui.label(theme::metadata_text("Scenario proposals and explicit approvals remain task scoped. Runs use the existing isolated Docker worker, bounded resources, private evidence store, preview relay, and cleanup path."));
}

fn capability_state(
    feature: ExperimentalFeature,
    profile: RuntimeProfile,
    docker: &DockerEnvironmentStatus,
    lock: Option<&ImageLock>,
    settings: &ExperimentalFeatureSettingsV1,
    reviewer_ready: bool,
) -> CapabilityState {
    if !cfg!(target_os = "linux") || docker.architecture.as_deref() != Some("amd64") {
        return CapabilityState::Unsupported;
    }
    let Some(lock) = lock else {
        return CapabilityState::Blocked;
    };
    let profile_key = if profile == RuntimeProfile::BrowserQa {
        "browser_qa"
    } else {
        "desktop_qa"
    };
    let Some(variant) = lock.profiles.get(profile_key).and_then(|variants| {
        variants
            .iter()
            .find(|variant| variant.platform == "linux/amd64")
    }) else {
        return CapabilityState::Blocked;
    };
    if variant.runtime_enabled {
        return CapabilityState::ReleaseEnabled;
    }
    if !settings.feature_enabled(feature)
        || !docker.engine_ready
        || docker.rootless != Some(true)
        || docker.desktop_vm
        || !reviewer_ready
        || !docker.available_images.contains(&variant.reference)
    {
        return CapabilityState::Blocked;
    }
    CapabilityState::ExperimentalReady
}

#[allow(clippy::too_many_arguments)] // Rendered explanation combines capability and local readiness inputs.
fn blocker(
    state: CapabilityState,
    feature: ExperimentalFeature,
    profile: RuntimeProfile,
    docker: &DockerEnvironmentStatus,
    lock: Option<&ImageLock>,
    settings: &ExperimentalFeatureSettingsV1,
    reviewer_is_ready: bool,
    platform: &str,
) -> String {
    match state {
        CapabilityState::ExperimentalReady => "Verified local digest is present on a ready rootless Linux amd64 Engine; task review, build/readiness evidence, approved scenarios, reviewer route, quotas, and screenshot validation are still checked before each run.".into(),
        CapabilityState::ReleaseEnabled => "Enabled by reviewed release image policy.".into(),
        CapabilityState::NotImplemented => "No runnable capability exists.".into(),
        CapabilityState::Unsupported => platform.into(),
        CapabilityState::Blocked if !cfg!(target_os = "linux") => "The QA driver is supported only on Linux amd64.".into(),
        CapabilityState::Blocked if docker.desktop_vm => "Requires a native Linux Docker Engine; Docker Desktop VM execution is unsupported.".into(),
        CapabilityState::Blocked if docker.rootless != Some(true) => "Requires a ready rootless Docker Engine; rootful and host execution fallbacks are forbidden.".into(),
        CapabilityState::Blocked if !docker.engine_ready => format!("Docker prerequisite is not ready: {}", docker.message),
        CapabilityState::Blocked => {
            let profile_name = if profile == RuntimeProfile::BrowserQa { "browser_qa" } else { "desktop_qa" };
            let present = lock.and_then(|lock| lock.profiles.get(profile_name)).and_then(|variants| variants.iter().find(|v| v.platform == "linux/amd64"));
            match present {
                None => "No immutable Linux amd64 QA image candidate is locked.".into(),
                Some(image) if !docker.available_images.contains(&image.reference) => format!("Exact image digest {} is not verified locally; explicitly pull this pinned reference from the operator guide.", image.digest),
                Some(_) if !settings.enabled => "Enable experimental mode on this machine to allow new experimental runs.".into(),
                Some(_) if !settings.feature_enabled(feature) => "Enable this feature to allow new experimental runs.".into(),
                Some(_) if !reviewer_is_ready => "Select a QA reviewer in Coding Tools and refresh its capability probe.".into(),
                Some(_) => "The QA reviewer route is not ready for experimental dispatch.".into(),
            }
        }
        CapabilityState::Ready => "Available.".into(),
    }
}

fn save(ui: &mut egui::Ui, settings: &ExperimentalFeatureSettingsV1) {
    if let Err(error) = experimental_features::save(settings) {
        ui.colored_label(
            theme::DANGER,
            format!("Could not save experimental settings: {error}"),
        );
    }
}
fn reviewer_ready(settings: &HarnessSettings) -> bool {
    let route = settings
        .work_routes
        .get(harness_settings::QA)
        .map(|route| route.harness.as_str())
        .or(settings.default_harness.as_deref());
    route
        .and_then(|id| settings.discovered.get(id))
        .is_some_and(|provider| {
            provider.supports_role_at(
                HarnessRole::RuntimeQaReviewer,
                harness_settings::current_unix_ms(),
            )
        })
}
fn testable(
    profile: &str,
    docker: &DockerEnvironmentStatus,
    lock: Option<&ImageLock>,
    reviewer_is_ready: bool,
) -> bool {
    if !cfg!(target_os = "linux")
        || docker.architecture.as_deref() != Some("amd64")
        || !docker.engine_ready
        || docker.rootless != Some(true)
        || docker.desktop_vm
        || !reviewer_is_ready
    {
        return false;
    }
    lock.and_then(|lock| lock.profiles.get(profile))
        .and_then(|variants| {
            variants
                .iter()
                .find(|variant| variant.platform == "linux/amd64")
        })
        .is_some_and(|variant| docker.available_images.contains(&variant.reference))
}
