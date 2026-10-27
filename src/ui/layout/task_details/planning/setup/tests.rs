use super::*;

fn task(
    id: &str,
    state: crate::persistence::setup_tasks::State,
) -> crate::persistence::setup_tasks::LocalTask {
    crate::persistence::setup_tasks::LocalTask {
        schema_version: 1,
        id: id.into(),
        title: "Synthetic setup task".into(),
        explanation: "Synthetic setup observation.".into(),
        next_step: "Review this setup task.".into(),
        dependencies: Vec::new(),
        blocked_by: Vec::new(),
        state,
        created_at_unix_ms: 1,
        observed_at_unix_ms: 1,
        scan_generation: 1,
        verified_at_unix_ms: None,
        verified_result: None,
        attempts: Vec::new(),
        events: Vec::new(),
        legacy_source_id: None,
        superseded_by: Vec::new(),
    }
}

#[test]
fn interrupted_and_unsupported_states_never_suggest_automatic_resume() {
    use crate::persistence::setup_tasks::State;

    assert!(guide::status_message(State::Interrupted).contains("Check again"));
    let unavailable = guide::status_message(State::Unsupported);
    assert!(unavailable.contains("Unavailable"));
    assert!(unavailable.contains("not a ready state"));
}

#[test]
fn windows_guide_names_wsl2_and_uses_official_vendor_links() {
    use crate::app::setup_attention::catalog::HostKind;
    use crate::persistence::setup_tasks::State;

    let task = task("setup:docker:desktop", State::ReadyToFix);
    assert!(
        guide::steps(&task, HostKind::Windows)
            .iter()
            .any(|step| step.contains("WSL 2"))
    );
    let links = guide::help_links(&task, HostKind::Windows);
    assert!(
        links
            .iter()
            .any(|(_, url)| url.contains("docs.docker.com/desktop/features/wsl"))
    );
    assert!(
        links
            .iter()
            .any(|(_, url)| url.contains("learn.microsoft.com/windows/wsl/install"))
    );
}

#[test]
fn macos_docker_help_does_not_claim_windows_or_native_linux_parity() {
    use crate::app::setup_attention::catalog::HostKind;
    use crate::persistence::setup_tasks::State;

    let task = task("setup:docker:start", State::Detected);
    assert!(
        guide::steps(&task, HostKind::MacOs)
            .iter()
            .any(|step| step.contains("Applications"))
    );
    let links = guide::help_links(&task, HostKind::MacOs);
    assert_eq!(links.len(), 1);
    assert!(links[0].1.contains("mac-install"));
}

#[test]
fn macos_desktop_guide_covers_finder_first_run_and_fail_closed_vm_resources() {
    use crate::app::setup_attention::catalog::HostKind;
    use crate::persistence::setup_tasks::State;

    let desktop = task("setup:docker:desktop", State::ReadyToFix);
    let desktop_steps = guide::steps(&desktop, HostKind::MacOs);
    assert!(desktop_steps.iter().any(|step| step.contains("processor")));
    assert!(
        desktop_steps
            .iter()
            .any(|step| step.contains("Applications"))
    );
    assert!(desktop_steps.iter().any(|step| step.contains("license")));
    assert!(desktop_steps.iter().any(|step| step.contains("Gatekeeper")));

    let disk = task("setup:docker:disk", State::ReadyToFix);
    assert!(
        guide::steps(&disk, HostKind::MacOs)
            .iter()
            .any(|step| step.contains("Mac host free space alone is not proof"))
    );
    assert!(
        guide::help_links(&disk, HostKind::MacOs)
            .iter()
            .any(|(_, url)| url.contains("settings-and-maintenance/settings"))
    );
    let endpoint = task("setup:docker:endpoint", State::ReadyToFix);
    assert!(
        guide::steps(&endpoint, HostKind::MacOs)
            .iter()
            .any(|step| step.contains("security warnings"))
    );
    assert!(
        guide::help_links(&endpoint, HostKind::MacOs)
            .iter()
            .any(|(_, url)| url.contains("mac-install"))
    );
    let limits = task("setup:docker:limits", State::ReadyToFix);
    assert!(
        guide::steps(&limits, HostKind::MacOs)
            .iter()
            .any(|step| step.contains("coding tasks stay paused"))
    );
}

#[test]
fn only_unblocked_general_images_offer_the_typed_repair() {
    use crate::persistence::setup_tasks::State;

    let mut image = task(
        "setup:images:general:amd64:aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
        State::ReadyToFix,
    );
    assert!(supports_repair(&image));
    image.blocked_by.push("setup:docker:rootless".into());
    assert!(!supports_repair(&image));
    image.blocked_by.clear();
    image.state = State::Unsupported;
    assert!(!supports_repair(&image));
    image.state = State::ReadyToFix;
    image.id = "setup:images:general:unknown:aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa".into();
    assert!(!supports_repair(&image));
}

#[test]
fn image_approval_and_progress_states_keep_the_repair_panel_visible() {
    use crate::persistence::setup_tasks::State;

    let mut image = task(
        "setup:images:general:amd64:aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
        State::NeedsApproval,
    );
    assert!(!supports_repair(&image));
    assert!(should_paint_image_preparation(&image));
    image.state = State::InProgress;
    assert!(should_paint_image_preparation(&image));
    image.state = State::AwaitingVerification;
    assert!(should_paint_image_preparation(&image));
    image.state = State::Verified;
    assert!(!should_paint_image_preparation(&image));
    image.state = State::BlockedByPrerequisite;
    image.blocked_by.push("setup:docker:rootless".into());
    assert!(!should_paint_image_preparation(&image));
}

#[test]
fn setup_details_stack_at_compact_widths_and_split_on_desktop() {
    for (width, split) in [
        (360.0, false),
        (900.0, false),
        (1280.0, true),
        (1600.0, true),
    ] {
        assert_eq!(uses_split_panes(width), split, "viewport width {width}");
    }
}
