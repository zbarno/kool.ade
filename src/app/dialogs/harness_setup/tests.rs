use super::*;
use crate::persistence::harness_settings::HarnessSettings;

#[cfg(unix)]
#[test]
fn manual_executable_path_requires_execute_permission_and_canonicalizes() {
    use std::os::unix::fs::PermissionsExt;

    let root = std::env::temp_dir().join(format!("harness-path-{}", uuid::Uuid::new_v4()));
    std::fs::create_dir_all(&root).unwrap();
    let binary = root.join("pi");
    std::fs::write(&binary, "#!/bin/sh\nexit 0\n").unwrap();
    std::fs::set_permissions(&binary, std::fs::Permissions::from_mode(0o644)).unwrap();
    assert!(!is_executable_file(&binary));
    std::fs::set_permissions(&binary, std::fs::Permissions::from_mode(0o755)).unwrap();
    assert!(is_executable_file(&binary.canonicalize().unwrap()));
    assert!(!is_executable_file(&root));
    let _ = std::fs::remove_dir_all(root);
}

fn report(ok: bool) -> HarnessProbe {
    HarnessProbe {
        id: "pi".into(),
        status: if ok { "pi 0.30.1" } else { "pi (unavailable)" }.into(),
        diagnostic: if ok {
            None
        } else {
            Some("login required".into())
        },
        executable: Some("/example/bin/pi".into()),
        version: ok.then(|| "0.30.1".into()),
        ready: ok,
        models: vec!["configured-model".into()],
        default_model: ok.then(|| "configured-model".into()),
        model_catalog_fresh: true,
        configuration_required: false,
        authentication: if ok {
            crate::execution::contract::ReadinessState::Ready
        } else {
            crate::execution::contract::ReadinessState::Unknown
        },
        implementation_available: true,
    }
}

mod discovery;
mod persistence;
mod ui;

fn bare_dialog(settings: HarnessSettings) -> DlgHarnessSetup {
    DlgHarnessSetup {
        settings,
        probe_view: ProbeView::Pending,
        feedback: None,
        section: HarnessSettingsSection::Tools,
        manual_path_drafts: std::collections::BTreeMap::new(),
        probe_rx: None,
        scan_generation: 1,
        scan_cancel: None,
        runtime_available: None,
        scan_results: Vec::new(),
        probe_results: Default::default(),
        running_providers: Default::default(),
        first_scan_pending: false,
    }
}

#[test]
fn rejecting_default_selection_explains_instead_of_silently_no_opping() {
    let mut settings = HarnessSettings::default();
    super::discovery::apply_probe_results(&mut settings, &[report(false)], None, 1);
    let mut dialog = bare_dialog(settings);
    dialog.select_default("pi");
    assert_eq!(dialog.settings.default_harness.as_deref(), None);
    let Some((ok, message, _)) = &dialog.feedback else {
        panic!("expected rejection feedback");
    };
    assert!(!ok);
    assert!(message.contains("Refresh Coding tools"));
}

#[test]
fn rejecting_work_route_selection_explains_instead_of_silently_no_opping() {
    let mut settings = HarnessSettings::default();
    super::discovery::apply_probe_results(&mut settings, &[report(false)], None, 1);
    let mut dialog = bare_dialog(settings);
    dialog.select_work_route("planning", "pi");
    assert!(!dialog.settings.work_routes.contains_key("planning"));
    let Some((ok, message, _)) = &dialog.feedback else {
        panic!("expected rejection feedback");
    };
    assert!(!ok);
    assert!(message.contains("Refresh Coding tools"));
}

#[test]
fn machine_probe_messages_persist_silently_without_raising_any_banner() {
    let root = std::env::temp_dir().join(format!("harness-quiet-persist-{}", uuid::Uuid::new_v4()));
    std::fs::create_dir_all(&root).unwrap();
    let settings_path = root.join("harness-settings.json");
    let (wrote_disk, raised_banner) =
        crate::persistence::harness_settings::with_settings_path(settings_path.clone(), || {
            let mut dialog = bare_dialog(HarnessSettings::default());
            dialog.apply_probe_message(super::discovery::ProbeMessage::Runtime {
                generation: 1,
                implementation_available: true,
            });
            dialog.apply_probe_message(super::discovery::ProbeMessage::Finished { generation: 1 });
            (
                settings_path.exists(),
                dialog.feedback.as_ref().is_some_and(|(ok, _, _)| *ok),
            )
        });
    assert!(wrote_disk, "machine probe flow must still persist");
    assert!(
        !raised_banner,
        "machine probe flow must never raise the saved banner"
    );
    let _ = std::fs::remove_dir_all(root);
}

#[test]
fn operator_feedback_expires_after_its_visibility_window() {
    let mut dialog = bare_dialog(HarnessSettings::default());
    let now = crate::persistence::harness_settings::current_unix_ms();
    dialog.feedback = Some((
        true,
        "stale".into(),
        now.saturating_sub(super::FEEDBACK_VISIBLE_FOR_MS + 1),
    ));
    dialog.drop_expired_feedback();
    assert!(dialog.feedback.is_none());
    dialog.feedback = Some((false, "fresh".into(), now));
    dialog.drop_expired_feedback();
    assert!(dialog.feedback.is_some());
}
