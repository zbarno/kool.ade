use super::*;

#[test]
fn coding_settings_sections_open_on_tools_and_expose_routing_categories() {
    assert_eq!(
        HarnessSettingsSection::default(),
        HarnessSettingsSection::Tools
    );
    let mut dialog = DlgHarnessSetup {
        settings: HarnessSettings::default(),
        probe_view: ProbeView::Complete,
        feedback: None,
        section: HarnessSettingsSection::Tools,
        manual_path_drafts: std::collections::BTreeMap::new(),
        probe_rx: None,
        scan_generation: 0,
        scan_cancel: None,
        runtime_available: None,
        scan_results: Vec::new(),
    };
    let ctx = egui::Context::default();
    let frame = |ctx: &egui::Context, dialog: &mut DlgHarnessSetup, events| {
        let mut output = ctx.run_ui(
            egui::RawInput {
                screen_rect: Some(egui::Rect::from_min_size(
                    egui::Pos2::ZERO,
                    egui::vec2(900.0, 700.0),
                )),
                events,
                ..Default::default()
            },
            |ui| {
                paint::paint_harness_setup_card(ui, dialog);
            },
        );
        output.textures_delta.clear();
        output
    };
    let position = |output: &egui::FullOutput, label: &str| {
        output
            .shapes
            .iter()
            .find_map(|shape| match &shape.shape {
                egui::Shape::Text(text) if text.galley.text() == label => {
                    Some(text.pos + text.galley.mesh_bounds.center().to_vec2())
                }
                _ => None,
            })
            .expect(label)
    };
    let click = |ctx: &egui::Context, dialog: &mut DlgHarnessSetup, pos: egui::Pos2| {
        for pressed in [true, false] {
            frame(
                ctx,
                dialog,
                vec![
                    egui::Event::PointerMoved(pos),
                    egui::Event::PointerButton {
                        pos,
                        button: egui::PointerButton::Primary,
                        pressed,
                        modifiers: Default::default(),
                    },
                ],
            );
        }
    };
    let output = frame(&ctx, &mut dialog, vec![]);
    assert!(output.shapes.iter().any(|shape| matches!(
        &shape.shape,
        egui::Shape::Text(text) if text.galley.text() == "Available coding tools"
    )));
    click(&ctx, &mut dialog, position(&output, "Models & routing"));
    assert_eq!(dialog.section, HarnessSettingsSection::Routing);
    let output = frame(&ctx, &mut dialog, vec![]);
    for label in [
        "Models & routing",
        "Implementation",
        "QA / Verification",
        "Documentation",
    ] {
        assert!(
            output.shapes.iter().any(|shape| matches!(
                &shape.shape,
                egui::Shape::Text(text) if text.galley.text() == label
            )),
            "missing section content: {label}"
        );
    }
    click(&ctx, &mut dialog, position(&output, "Coding tools"));
    assert_eq!(dialog.section, HarnessSettingsSection::Tools);
    let output = frame(&ctx, &mut dialog, vec![]);
    assert!(output.shapes.iter().any(|shape| matches!(
        &shape.shape,
        egui::Shape::Text(text) if text.galley.text() == "Available coding tools"
    )));
}

#[test]
fn codex_settings_show_models_when_the_docker_execution_session_is_unavailable() {
    use crate::persistence::harness_settings::DetectedHarness;

    let models = vec![
        "gpt-6.1-sol".into(),
        "gpt-6-astra".into(),
        "gpt-6-sol".into(),
        "gpt-6-luna".into(),
        "gpt-5.6-sol".into(),
    ];
    let mut dialog = DlgHarnessSetup {
        settings: HarnessSettings {
            default_harness: Some("codex".into()),
            discovered: std::collections::BTreeMap::from([(
                "codex".into(),
                DetectedHarness {
                    status: "codex ready".into(),
                    version: Some("0.162.0".into()),
                    executable: Some("/example/bin/codex".into()),
                    diagnostic: None,
                    ready: true,
                    models,
                    default_model: Some("gpt-6.1-sol".into()),
                    model_catalog_fresh: true,
                    configuration_required: false,
                    implementation_available: false,
                    checked_at_unix_ms: crate::persistence::harness_settings::current_unix_ms(),
                    capability_snapshot: Some(codex_catalog_snapshot()),
                },
            )]),
            ..HarnessSettings::default()
        },
        probe_view: ProbeView::Complete,
        feedback: None,
        section: HarnessSettingsSection::Tools,
        manual_path_drafts: std::collections::BTreeMap::new(),
        probe_rx: None,
        scan_generation: 0,
        scan_cancel: None,
        runtime_available: None,
        scan_results: Vec::new(),
    };
    let ctx = egui::Context::default();
    let mut output = ctx.run_ui(
        egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(
                egui::Pos2::ZERO,
                egui::vec2(900.0, 700.0),
            )),
            ..Default::default()
        },
        |ui| {
            paint::paint_harness_setup_card(ui, &mut dialog);
        },
    );
    let visible = output
        .shapes
        .iter()
        .filter_map(|shape| match &shape.shape {
            egui::Shape::Text(text) => Some(text.galley.text().to_owned()),
            _ => None,
        })
        .collect::<Vec<_>>();
    assert!(
        visible
            .iter()
            .any(|text| text == "Models reported by this CLI")
    );
    assert!(
        visible
            .iter()
            .any(|text| { text == "gpt-6.1-sol, gpt-6-astra, gpt-6-sol, gpt-6-luna, gpt-5.6-sol" })
    );
    assert!(
        visible
            .iter()
            .any(|text| text == "CLI default: gpt-6.1-sol")
    );
    output.textures_delta.clear();

    dialog.section = HarnessSettingsSection::Routing;
    let mut routing = ctx.run_ui(
        egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(
                egui::Pos2::ZERO,
                egui::vec2(900.0, 700.0),
            )),
            ..Default::default()
        },
        |ui| {
            paint::paint_harness_setup_card(ui, &mut dialog);
        },
    );
    let routing_text = routing
        .shapes
        .iter()
        .filter_map(|shape| match &shape.shape {
            egui::Shape::Text(text) => Some(text.galley.text().to_owned()),
            _ => None,
        })
        .collect::<Vec<_>>();
    assert!(routing_text.iter().any(|text| {
        text.contains(
            "Repository commands run through Kool.ad/e's application-owned Docker ExecutionSession",
        )
    }));
    routing.textures_delta.clear();
}

fn codex_catalog_snapshot() -> crate::execution::contract::HarnessCapabilitySnapshotV1 {
    use crate::execution::contract::{
        HarnessCapabilitySnapshotV1, HarnessRole, ModeReadinessV1, ReadinessState, RoleReadinessV1,
    };

    HarnessCapabilitySnapshotV1 {
        schema_version: 1,
        provider_id: "codex".into(),
        probe_id: "123e4567-e89b-12d3-a456-426614174000".into(),
        probe_cancellation_generation: 1,
        probe_sequence: 1,
        cli_binary: ReadinessState::Ready,
        cli_version: Some("0.162.0".into()),
        authentication: ReadinessState::Ready,
        model_catalog: ReadinessState::Ready,
        scriptable_invocation: ReadinessState::Unknown,
        docker_mcp_bridge: ReadinessState::Unavailable,
        execution_target: ReadinessState::Unavailable,
        failure: None,
        modes: crate::harness::ExecutionMode::ALL
            .into_iter()
            .map(|mode| ModeReadinessV1 {
                mode,
                available: ReadinessState::Unknown,
                failure: None,
            })
            .collect(),
        roles: HarnessRole::ALL
            .into_iter()
            .map(|role| RoleReadinessV1 {
                role,
                available: ReadinessState::Unknown,
                failure: None,
            })
            .collect(),
    }
}
