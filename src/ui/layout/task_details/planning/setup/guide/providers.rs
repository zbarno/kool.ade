use crate::{app::setup_attention::catalog::ProviderIdV1, persistence::setup_tasks::LocalTask};

pub(super) fn provider_task(task: &LocalTask) -> Option<(ProviderIdV1, &str)> {
    let suffix = task.id.strip_prefix("setup:cli:")?;
    let (provider, stage) = suffix.split_once(':')?;
    Some((ProviderIdV1::from_probe_id(provider)?, stage))
}

pub(super) fn provider_progress(
    task: &LocalTask,
) -> Option<(ProviderIdV1, &'static str, &'static str)> {
    let (provider, stage) = provider_task(task)?;
    let stage = match stage {
        "install" => "Provider installation",
        "path" => "Executable path",
        "version" => "Supported provider version",
        "auth" => "Provider sign-in",
        "auth-verify" => "Sign-in verification",
        "models" => "Model availability",
        "capability" => "Safe setup capability",
        "verify" => "Fresh provider verification",
        _ => return None,
    };
    Some((provider, stage, task.state_label()))
}

pub(super) fn paint_progress(ui: &mut egui::Ui, task: &LocalTask, provider: ProviderIdV1) {
    ui.add_space(crate::ui::theme::spacing::S);
    ui.label(crate::ui::theme::section_heading(provider.display_name()));
    if let Some((_, current_stage, state)) = provider_progress(task) {
        ui.label(crate::ui::theme::section_heading(
            "Current provider setup stage",
        ));
        ui.horizontal_wrapped(|ui| {
            ui.label(current_stage);
            crate::ui::theme::badge(
                ui,
                state,
                crate::ui::theme::PANEL_ALT,
                crate::ui::theme::TEXT,
            );
        });
        ui.label("This status belongs to this setup task only; other provider stages are not marked complete here.");
    }
}

pub(super) fn provider_task_guidance(_provider: ProviderIdV1, stage: &str) -> &'static str {
    match stage {
        "install" => {
            "Kool.ad/e offers an app-owned, reviewed install only when a verified recipe supports this exact host. If the option is unavailable, follow the official instructions for this operating system, then rediscover the provider."
        }
        "path" => {
            "Check the executable path in Coding Tools, or return to automatic discovery. Then rediscover the provider."
        }
        "version" => {
            "Install a supported provider version using its official instructions, then rediscover it."
        }
        "auth" => {
            "Follow the provider-specific sign-in steps below. Complete browser or device approval on the vendor's own surface, then rediscover it."
        }
        "auth-verify" => {
            "Follow the provider-specific sign-in steps below. If this provider has no safe non-generative account check, Kool.ad/e will keep current access unverified."
        }
        "models" => {
            "Choose a model in the provider, then rediscover it. A configured model name alone does not prove account access."
        }
        "capability" => {
            "Kool.ad/e requires a verified context-only invocation for setup help. Update or reconfigure the provider, then rediscover it."
        }
        "verify" => {
            "Review the provider status in Coding Tools and rediscover after correcting the issue."
        }
        _ => {
            "Review the provider status in Coding Tools and rediscover after correcting the issue."
        }
    }
}

pub(super) fn provider_steps(provider: ProviderIdV1, stage: &str) -> Vec<&'static str> {
    if matches!(stage, "auth" | "auth-verify") {
        let mut steps = provider.authentication_steps().to_vec();
        steps.push(
            "Never paste passwords, tokens, API keys, one-time codes, or browser data into Kool.ad/e.",
        );
        if stage == "auth-verify" {
            steps.push(
                "Kool.ad/e may keep this task unverified when the provider has no safe account-status check.",
            );
        }
        return steps;
    }
    match stage {
        "install" => vec![
            "Review the Fix for me option. If it is unavailable, open the provider's official installation guide for this operating system.",
            "Approve only an exact app-owned recipe, or complete installation on the provider's own surface.",
            "Return to Coding Tools and choose Retry discovery.",
        ],
        "path" => vec![
            "Open Coding Tools and check the saved executable path.",
            "Choose a valid executable path or return to automatic discovery.",
            "Choose Retry discovery and review the new result.",
        ],
        "version" => vec![
            "Open the official provider installation guide.",
            "Install a release that supports Kool.ad/e's verified context-only mode.",
            "Return to Coding Tools and choose Retry discovery.",
        ],
        "auth" | "auth-verify" => unreachable!("authentication stages return above"),
        "models" => vec![
            "Open the provider's model setup guide and choose a model available to your account.",
            "Return to Coding Tools and choose Retry discovery.",
            "A model is ready only when the fresh provider result verifies it.",
        ],
        "capability" => vec![
            "Review the provider's current version and configuration.",
            "Kool.ad/e uses a restricted context-only setup role; project tools and host commands stay unavailable here.",
            "Choose Retry discovery after updating or reconfiguring the provider.",
        ],
        _ => vec![
            "Review the provider's redacted status in Coding Tools.",
            "Follow the provider's official guidance, then choose Check again.",
        ],
    }
}

pub(super) fn provider_links(
    provider: ProviderIdV1,
    stage: &str,
) -> Vec<(&'static str, &'static str)> {
    match stage {
        "install" | "path" | "version" => {
            vec![("Official installation guide", provider.installation_guide())]
        }
        "auth" | "auth-verify" => {
            vec![("Official sign-in guide", provider.authentication_guide())]
        }
        "models" => vec![("Official model guide", provider.model_guide())],
        "capability" | "verify" => vec![
            ("Official installation guide", provider.installation_guide()),
            ("Official sign-in guide", provider.authentication_guide()),
            ("Official model guide", provider.model_guide()),
        ],
        _ => Vec::new(),
    }
}
