//! Plain-language Computer Use orientation shown before pending consent.

const IMAGE_LOCK: &str = include_str!("../../../../../../containers/computer-use/images.lock.json");
const IMAGE_CATALOG: &str =
    include_str!("../../../../../../containers/computer-use/catalog-v1.json");
const TEST_WORKFLOW: [&str; 5] = [
    "1. Choose the task and confirm its source revision.",
    "2. Read the exact VM, app, network, operation, and resource request.",
    "3. Use synthetic data and approve only the task-bound request you intend.",
    "4. Pause or take control whenever the agent needs a person.",
    "5. Inspect the outcome; stop the VM to destroy its disposable overlay.",
];

pub(super) fn paint(ui: &mut egui::Ui) {
    ui.label(crate::ui::theme::section_heading("Computer Use onboarding"));
    ui.label(crate::ui::theme::helper_text(
        "Computer Use opens a separate, experimental Linux VM for a task. A VM does not guarantee that a signed-in account is safe, and running a browser does not mean a website permits automation.",
    ));

    ui.horizontal_wrapped(|ui| {
        ui.label(egui::RichText::new("Status terms").strong());
        ui.label("Ready: current evidence supports this operation.");
        ui.label("Experimental: use synthetic data; release evidence is incomplete.");
        ui.label("Unavailable: the app blocks this operation on this build or host.");
    });

    ui.group(|ui| {
        ui.label(egui::RichText::new("Test an app UI · Experimental").strong());
        ui.label(
            "Use a disposable test task and synthetic data. The current guest is offline and can launch only its signed, session-allowed apps.",
        );
        for step in TEST_WORKFLOW {
            ui.label(step);
        }
        ui.add_space(crate::ui::theme::spacing::S);
        ui.label(egui::RichText::new("Research a website · Unavailable").strong());
        ui.label(format!(
            "{} Do not copy browser cookies, log in, or use a host browser as a workaround.",
            internet_research_status()
        ));
        ui.label(egui::RichText::new("Resume a personal computer · Unavailable").strong());
        ui.label(
            "Persistent identity and credentials remain disabled until the separate security and human-login gates pass. A disposable task grant is not a personal-account grant.",
        );
    });

    ui.group(|ui| {
        ui.label(egui::RichText::new("Before you approve a VM").strong());
        ui.label(
            "Before approval, review the task, session, VM, apps, operations, network profile, and resource budget shown in the request card below.",
        );
        ui.label(
            "Files: no host folder is shared. A task file or export needs its own exact, one-use grant; clipboard sharing is off unless separately approved.",
        );
        ui.label(
            "Evidence: approved observations may return screenshots; the private journal keeps bounded activity metadata and evidence references. Never type a password, recovery code, payment detail, or other secret.",
        );
        ui.label(
            "Cleanup: stopping destroys this session's disposable overlay and VM process. The verified base image cache and private task/evidence records are separate and may remain.",
        );
        ui.label(
            "Interruption: pause the agent, take control, return control, revoke a permission, or stop the VM. If an action outcome is uncertain, the agent stays paused and the app never retries it automatically.",
        );
    });

    ui.hyperlink_to(
        "Read the Computer Use permissions, privacy, and recovery guide",
        "https://github.com/zbarno/kool.ade/blob/main/docs/computer-use-architecture.md",
    );
}

fn internet_research_status() -> String {
    let promoted = serde_json::from_str::<serde_json::Value>(IMAGE_LOCK)
        .ok()
        .and_then(|lock| lock.get("status")?.as_str().map(str::to_owned))
        .is_some_and(|status| status == "promoted");
    let protocol = serde_json::from_str::<serde_json::Value>(IMAGE_CATALOG)
        .ok()
        .and_then(|catalog| {
            catalog
                .get("images")?
                .as_array()?
                .first()?
                .get("companion_protocol_version")?
                .as_u64()
        });
    match (promoted, protocol) {
        (true, Some(1)) => "Unavailable: the promoted guest image uses companion protocol v1, which does not offer InternetResearch.".to_owned(),
        (true, Some(version)) => format!(
            "Experimental: the promoted guest image advertises companion protocol v{version}; its network profile still requires current host readiness and release acceptance."
        ),
        _ => "Unavailable: this build cannot verify a promoted guest image and compatible companion protocol.".to_owned(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn onboarding_copy_names_status_risk_permissions_and_controls() {
        assert_eq!(TEST_WORKFLOW.len(), 5);
        assert!(TEST_WORKFLOW[0].contains("source revision"));
        assert!(TEST_WORKFLOW[1].contains("network"));
        assert!(TEST_WORKFLOW[4].contains("disposable overlay"));
        let research = internet_research_status();
        assert!(research.contains("Unavailable"));
        assert!(research.contains("protocol v1"));
    }

    #[test]
    fn research_status_follows_promoted_catalog_and_protocol() {
        let status = internet_research_status();
        assert!(
            status.starts_with("Unavailable: the promoted guest image uses companion protocol v1")
        );
    }

    #[test]
    fn onboarding_renders_in_a_narrow_static_layout() {
        const WIDTH: f32 = 360.0;
        let context = egui::Context::default();
        let mut output = context.run_ui(
            egui::RawInput {
                screen_rect: Some(egui::Rect::from_min_size(
                    egui::Pos2::ZERO,
                    egui::vec2(WIDTH, 1_200.0),
                )),
                ..Default::default()
            },
            paint,
        );
        output.textures_delta.clear();
        assert!(!output.shapes.is_empty());
        let text = output
            .shapes
            .iter()
            .filter_map(|shape| match &shape.shape {
                egui::Shape::Text(text) => Some(text.galley.text().to_owned()),
                _ => None,
            })
            .collect::<Vec<_>>();
        let rendered = text.join(" ");
        for phrase in [
            "Computer Use onboarding",
            "Test an app UI",
            "Research a website",
            "Resume a personal computer",
            "Files: no host folder is shared",
            "Evidence:",
            "Cleanup:",
            "Interruption:",
            "protocol v1",
        ] {
            assert!(rendered.contains(phrase), "missing rendered text: {phrase}");
        }
        for shape in &output.shapes {
            if let egui::Shape::Text(text) = &shape.shape {
                assert!(
                    text.pos.x + text.galley.size().x <= WIDTH,
                    "text exceeded narrow viewport: {}",
                    text.galley.text()
                );
            }
        }
    }
}
