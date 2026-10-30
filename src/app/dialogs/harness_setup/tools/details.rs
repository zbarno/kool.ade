use super::super::{DlgHarnessSetup, ProbeView};
use crate::ui::theme;
use egui::{RichText, Ui};

pub(super) fn paint_tool_details(
    ui: &mut Ui,
    dialog: &mut DlgHarnessSetup,
    id: &str,
    harness: &crate::persistence::harness_settings::DetectedHarness,
) {
    if id == "pi" {
        super::super::pi_guide::paint(ui, dialog);
    }
    let now = crate::persistence::harness_settings::current_unix_ms();
    let fresh = harness.is_fresh_at(now);
    ui.label(theme::page_title(id));
    let availability = availability_label(
        id,
        harness,
        now,
        matches!(dialog.probe_view, ProbeView::Pending),
    );
    ui.label(
        RichText::new(availability).color(if availability == "Available" {
            theme::SUCCESS
        } else if availability.contains("unverified") || availability.starts_with("Checking") {
            theme::WARNING
        } else {
            theme::DANGER
        }),
    );
    if fresh && let Some(snapshot) = harness.capability_snapshot.as_ref() {
        use crate::execution::contract::ReadinessState;
        let state_label = |state| match state {
            ReadinessState::Ready => "ready",
            ReadinessState::Unavailable => "unavailable",
            ReadinessState::Unknown => "not checked",
        };
        ui.label(theme::metadata_text(format!(
            "Capabilities: CLI {}, authentication {}, model catalog {}, scriptable invocation {}",
            state_label(snapshot.cli_binary),
            state_label(snapshot.authentication),
            state_label(snapshot.model_catalog),
            state_label(snapshot.scriptable_invocation),
        )));
        ui.label(theme::metadata_text(format!(
            "Execution: Docker bridge {}, target {}",
            state_label(snapshot.docker_mcp_bridge),
            state_label(snapshot.execution_target),
        )));
    }
    ui.label(theme::helper_text(if fresh {
        format!("Status: {}", harness.status)
    } else {
        format!(
            "Last result: {}. Rediscover to refresh provider and model status.",
            harness.status
        )
    }));
    super::probe_status::paint_probe_status(ui, dialog, id);
    ui.label(theme::helper_text(format!(
        "Security boundary: {}",
        crate::harness::execution_security::summary_for_harness(id)
    )));
    if fresh && harness.ready && !harness.implementation_available {
        ui.label(theme::helper_text(implementation_guidance(id)));
    }
    if let Some(version) = &harness.version {
        ui.label(theme::helper_text(format!("Version: {version}")));
    }
    if super::super::has_current_model_catalog(
        id,
        harness,
        crate::persistence::harness_settings::current_unix_ms(),
    ) && !harness.models.is_empty()
    {
        ui.label(RichText::new("Models reported by this CLI").strong());
        ui.label(harness.models.join(", "));
        if let Some(model) = &harness.default_model {
            ui.label(theme::metadata_text(format!("CLI default: {model}")));
        }
    } else if harness.model_catalog_status() == crate::harness::discovery::ModelCatalogStatus::Stale
    {
        ui.label(theme::metadata_text(
            "Last-known models are stale and cannot be selected until discovery refreshes them.",
        ));
        ui.label(harness.models.join(", "));
    } else {
        ui.label(theme::metadata_text(
            "This CLI model catalog is unavailable or has not been checked; refresh discovery before selecting a model.",
        ));
    }
    let configured = dialog.settings.manual_executable_paths.get(id).cloned();
    let current_path = configured.as_deref().or(harness.executable.as_deref());
    ui.label("Current executable");
    ui.label(
        RichText::new(current_path.unwrap_or("not found"))
            .monospace()
            .size(11.0)
            .color(theme::TEXT_DIM),
    );
    ui.label(if configured.is_some() {
        "Source: Manual"
    } else {
        "Source: Auto-detected"
    });
    if configured
        .as_deref()
        .is_some_and(|path| match std::path::Path::new(path).canonicalize() {
            Ok(canonical) => !super::super::is_executable_file(&canonical),
            Err(_) => true,
        })
    {
        ui.colored_label(theme::DANGER, "Invalid path");
    }
    if ui
        .add_enabled(
            can_select_default(id, harness, now),
            egui::RadioButton::new(
                dialog.settings.default_harness.as_deref() == Some(id),
                "Default tool",
            ),
        )
        .clicked()
    {
        dialog.select_default(id);
    }
    ui.add_space(theme::spacing::S);
    ui.label(RichText::new("Executable path").strong());
    let mut draft = dialog
        .manual_path_drafts
        .get(id)
        .cloned()
        .unwrap_or_else(|| {
            configured
                .clone()
                .or_else(|| harness.executable.clone())
                .unwrap_or_default()
        });
    ui.add(
        egui::TextEdit::singleline(&mut draft)
            .desired_width(f32::INFINITY)
            .hint_text("/path/to/cli"),
    );
    dialog
        .manual_path_drafts
        .insert(id.to_owned(), draft.clone());
    ui.horizontal_wrapped(|ui| {
        if ui.button(format!("Retry {id} discovery")).clicked() {
            dialog.retry_provider(id);
        }
        if ui.button("Use Path").clicked() {
            dialog.set_manual_path(id, draft.clone());
        }
        if configured.is_some() && ui.button("Return to auto-detect").clicked() {
            dialog.reset_manual_path(id);
        }
    });
    if let Some(diagnostic) = &harness.diagnostic {
        ui.colored_label(theme::DANGER, diagnostic);
    }
    paint_provider_guidance(ui, dialog, id, harness);
}

pub(super) fn availability_label(
    id: &str,
    harness: &crate::persistence::harness_settings::DetectedHarness,
    now_unix_ms: u64,
    checking: bool,
) -> &'static str {
    if !harness.is_fresh_at(now_unix_ms) {
        return if checking {
            "Checking this tool"
        } else {
            "Discovery expired; refresh required"
        };
    }
    if harness.capability_snapshot.is_none() {
        return "Capability snapshot unavailable; refresh required";
    }
    if harness.configuration_required {
        return "Sign-in required";
    }
    if crate::harness::execution_security::task_execution_block_reason(id).is_some() {
        return "Provider execution unavailable";
    }
    if !super::super::has_current_provider_capabilities(id, harness, now_unix_ms) {
        return "Authentication unverified";
    }
    if !super::super::has_current_model_catalog(id, harness, now_unix_ms)
        || !harness.has_any_verified_model_at(id, now_unix_ms)
    {
        return "Authenticated; model access unverified";
    }
    if harness.verified_default_model_at(id, now_unix_ms).is_none() {
        return "Choose a model; CLI default is unverified";
    }
    let planning_analyst = crate::execution::contract::HarnessRole::PlanningAnalyst;
    if !harness.supports_role_at(planning_analyst, now_unix_ms)
        && !harness.supports_context_only_planning_at(now_unix_ms)
    {
        return "Context-only mode unavailable";
    }
    "Available"
}

fn can_select_default(
    id: &str,
    harness: &crate::persistence::harness_settings::DetectedHarness,
    now_unix_ms: u64,
) -> bool {
    availability_label(id, harness, now_unix_ms, false) == "Available"
}

fn paint_provider_guidance(
    ui: &mut Ui,
    dialog: &mut DlgHarnessSetup,
    id: &str,
    harness: &crate::persistence::harness_settings::DetectedHarness,
) {
    let Some(provider) = crate::app::setup_attention::catalog::ProviderIdV1::from_probe_id(id)
    else {
        return;
    };
    let host = crate::app::setup_attention::catalog::HostKind::current();
    ui.label(RichText::new("Provider setup tasks").strong());
    ui.label("Kool.ad/e checks the provider again after you finish. Only a reviewed, host-specific installer appears as Fix for me; otherwise follow the official guide. Sign in on the provider's own surface. Kool.ad/e never collects passwords, tokens, API keys, or browser data.");
    let authentication_ready = harness.has_current_authenticated_capability(
        id,
        crate::persistence::harness_settings::current_unix_ms(),
    );
    if !authentication_ready {
        ui.collapsing("Sign-in steps", |ui| {
            for step in provider.authentication_steps() {
                ui.label(*step);
            }
        });
    }
    ui.label(theme::helper_text(provider.install_note(host)));
    ui.label(theme::helper_text(provider.install_channel_note(host)));
    ui.label("Man.ager helps plan your project, answer questions and coordinate work. Your provider's account and usage charges still apply.");
    if ui.add_enabled(
        dialog.onboarding_model(id).is_some(),
        egui::Button::new("Use as Man.ager and setup helper"),
    ).on_disabled_hover_text("Check this tool again after signing in and selecting a default model. It must support verified planning help.").clicked() {
        dialog.select_onboarding_manager(id);
    }
    ui.label(theme::helper_text("This saves your Manager choice and setup helper. Repository work remains gated by the separate Manager and safe-execution checks; selecting a provider does not grant access."));
    let selected = dialog.settings.bootstrap_provider.as_deref() == Some(id);
    if ui
        .add(egui::RadioButton::new(
            selected,
            "Prioritize this provider for setup tasks",
        ))
        .clicked()
    {
        dialog.select_bootstrap_provider(id);
    }
    if selected && ui.button("Clear setup task priority").clicked() {
        dialog.clear_bootstrap_provider();
    }
    ui.label(theme::helper_text(
        "This selects the provider for provider-specific setup tasks only. Conversations still follow Default tool and work-type routes, with Manager authoritative. If unset, the selected Default tool provider is used for these tasks.",
    ));
    ui.horizontal_wrapped(|ui| {
        ui.hyperlink_to("Installation guide", provider.installation_guide());
        ui.hyperlink_to("Sign-in guide", provider.authentication_guide());
        ui.hyperlink_to("Model guide", provider.model_guide());
    });
}

fn implementation_guidance(id: &str) -> &'static str {
    crate::harness::execution_security::task_execution_block_reason(id).unwrap_or(
        "Repository execution is unavailable until Kool.ad/e has a ready Docker execution session for this tool. Context-only planning can continue without repository tools. The approved runtime image must also be available.",
    )
}

#[cfg(test)]
#[path = "details/tests.rs"]
mod tests;
