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
fn selected_provider_tasks_show_official_links_and_manual_secret_safe_steps() {
    use crate::app::setup_attention::catalog::{HostKind, ProviderIdV1};
    use crate::persistence::setup_tasks::State;

    for (id, provider) in [
        ("pi", ProviderIdV1::Pi),
        ("codex", ProviderIdV1::Codex),
        ("claude", ProviderIdV1::Claude),
        ("opencode", ProviderIdV1::OpenCode),
        ("copilot", ProviderIdV1::Copilot),
        ("antigravity", ProviderIdV1::Antigravity),
    ] {
        let install = task(&format!("setup:cli:{id}:install"), State::ReadyToFix);
        let install_links = guide::help_links(&install, HostKind::Windows);
        assert_eq!(install_links[0].1, provider.installation_guide(), "{id}");
        assert!(
            guide::rendered_steps(&install, HostKind::Windows)
                .iter()
                .any(|step| step.contains("official installation guide"))
        );

        let auth = task(
            &format!("setup:cli:{id}:auth-verify"),
            State::NeedsVerification,
        );
        let auth_links = guide::help_links(&auth, HostKind::MacOs);
        assert_eq!(auth_links[0].1, provider.authentication_guide(), "{id}");
        assert!(guide::provider_task_guidance(provider, "auth-verify").contains("unverified"));
        assert!(
            guide::rendered_steps(&auth, HostKind::MacOs)
                .iter()
                .any(|step| step.contains("passwords, tokens, API keys"))
        );
    }
}

#[test]
fn provider_sign_in_steps_name_vendor_flows_without_collecting_credentials() {
    use crate::app::setup_attention::catalog::{HostKind, ProviderIdV1};
    use crate::persistence::setup_tasks::State;

    for (id, provider, expected) in [
        ("pi", ProviderIdV1::Pi, "/login"),
        ("codex", ProviderIdV1::Codex, "codex"),
        ("claude", ProviderIdV1::Claude, "claude"),
        ("opencode", ProviderIdV1::OpenCode, "opencode auth login"),
        ("copilot", ProviderIdV1::Copilot, "/login"),
        ("antigravity", ProviderIdV1::Antigravity, "agy"),
    ] {
        let task = task(&format!("setup:cli:{id}:auth"), State::ReadyToFix);
        let steps = guide::rendered_steps(&task, HostKind::Linux);
        assert!(
            steps.iter().any(|step| step.contains(expected)),
            "{id} sign-in guidance should name its vendor flow"
        );
        assert!(guide::provider_task_guidance(provider, "auth").contains("vendor's own surface"));
    }
}

#[test]
fn provider_setup_progress_uses_only_the_exact_task_id_and_persisted_state() {
    use crate::persistence::setup_tasks::State;

    for (stage, label) in [
        ("install", "Provider installation"),
        ("path", "Executable path"),
        ("version", "Supported provider version"),
        ("auth", "Provider sign-in"),
        ("auth-verify", "Sign-in verification"),
        ("models", "Model availability"),
        ("capability", "Safe setup capability"),
        ("verify", "Fresh provider verification"),
    ] {
        let current = task(&format!("setup:cli:pi:{stage}"), State::ReadyToFix);
        assert_eq!(
            guide::provider_progress(&current),
            Some(("Pi", label, "Ready to fix")),
            "{stage}"
        );
    }

    let verified = task("setup:cli:codex:models", State::Verified);
    assert_eq!(
        guide::provider_progress(&verified),
        Some(("Codex", "Model availability", "Verified"))
    );

    for id in [
        "setup:cli:install",
        "setup:cli:codex:models:extra",
        "setup:cli:unknown:auth",
    ] {
        assert_eq!(
            guide::provider_progress(&task(id, State::Verified)),
            None,
            "{id}"
        );
    }
}

#[test]
fn unselected_provider_chooser_has_no_provider_specific_install_recipe() {
    use crate::app::setup_attention::catalog::HostKind;
    use crate::persistence::setup_tasks::State;

    let chooser = task("setup:cli:install", State::ReadyToFix);
    assert!(guide::help_links(&chooser, HostKind::Linux).is_empty());
    assert!(
        guide::rendered_steps(&chooser, HostKind::Linux)
            .iter()
            .any(|step| step.contains("Choose a supported coding agent"))
    );
}

#[test]
fn only_unblocked_supported_repairs_offer_the_typed_action() {
    use crate::persistence::setup_tasks::State;

    let mut image = task("setup:images:general:amd64", State::ReadyToFix);
    assert!(supports_repair(&image, false));
    image.blocked_by.push("setup:docker:rootless".into());
    assert!(!supports_repair(&image, false));
    image.blocked_by.clear();
    image.state = State::Unsupported;
    assert!(!supports_repair(&image, false));
    image.state = State::ReadyToFix;
    image.id = "setup:images:general:unknown".into();
    assert!(!supports_repair(&image, false));

    let mut claude = task("setup:cli:claude:install", State::ReadyToFix);
    assert!(!supports_repair(&claude, false));
    assert!(supports_repair(&claude, true));
    claude.blocked_by.push("setup:host:linux".into());
    assert!(!supports_repair(&claude, true));
    claude.blocked_by.clear();
    claude.state = State::Unsupported;
    assert!(!supports_repair(&claude, true));
}

#[test]
fn claude_install_review_is_limited_to_active_supported_task_states() {
    use crate::persistence::setup_tasks::State;

    let mut claude = task("setup:cli:claude:install", State::ReadyToFix);
    assert!(should_paint_claude_install(&claude, true));
    assert!(!should_paint_claude_install(&claude, false));
    claude.state = State::NeedsApproval;
    assert!(should_paint_claude_install(&claude, true));
    claude.state = State::InProgress;
    assert!(should_paint_claude_install(&claude, true));
    claude.state = State::AwaitingVerification;
    assert!(should_paint_claude_install(&claude, true));
    claude.state = State::Verified;
    assert!(!should_paint_claude_install(&claude, true));
    claude.state = State::ReadyToFix;
    claude.blocked_by.push("setup:host:linux".into());
    assert!(!should_paint_claude_install(&claude, true));
    claude.id = "setup:cli:codex:install".into();
    claude.blocked_by.clear();
    assert!(!should_paint_claude_install(&claude, true));
}

#[test]
fn image_approval_and_progress_states_keep_the_repair_panel_visible() {
    use crate::persistence::setup_tasks::State;

    let mut image = task("setup:images:general:amd64", State::NeedsApproval);
    assert!(!supports_repair(&image, false));
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
