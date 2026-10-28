use super::*;

/// Thin, actionable nudge shown atop the task conversation when coding
/// work cannot be routed to a healthy CLI. Reads the same persisted
/// settings the router consumes (a few kilobytes, page-cache hot; task
/// detail repaints at most a few times per second), so the advice can
/// never lag what the router enforces. Selection itself stays explicit:
/// the nudge only carries the operator to the chooser.
pub(super) fn paint(ui: &mut egui::Ui) {
    let (settings, _) = crate::persistence::harness_settings::load();
    let Some(notice) = crate::app::root::cli_routing_notice(&settings) else {
        return;
    };
    ui.horizontal_wrapped(|ui| {
        ui.colored_label(theme::WARNING, notice.message);
        let label = match notice.ready_provider {
            Some(ref provider) => format!("{provider} is ready — choose a coding CLI"),
            None => "Choose a coding CLI".to_owned(),
        };
        if ui
            .add(egui::Button::new(egui::RichText::new(label).strong()).fill(theme::ACTION))
            .clicked()
        {
            ui.ctx().data_mut(|data| {
                data.insert_temp(egui::Id::new(CODING_SETTINGS_REQUEST_ID), true);
            });
        }
    });
    ui.add_space(theme::spacing::M);
}

const CODING_SETTINGS_REQUEST_ID: &str = "koolade_open_coding_settings";

#[cfg(test)]
mod tests {
    use super::*;
    use crate::execution::contract::{
        HarnessCapabilitySnapshotV1, ModeReadinessV1, ReadinessState, RoleReadinessV1,
    };
    use crate::persistence::harness_settings::DetectedHarness;

    fn snapshot(provider: &str) -> HarnessCapabilitySnapshotV1 {
        HarnessCapabilitySnapshotV1 {
            schema_version: 1,
            provider_id: provider.into(),
            probe_id: uuid::Uuid::new_v4().hyphenated().to_string(),
            probe_cancellation_generation: 1,
            probe_sequence: 1,
            cli_binary: ReadinessState::Ready,
            cli_version: Some("1.0.0".into()),
            authentication: ReadinessState::Ready,
            model_catalog: ReadinessState::Ready,
            scriptable_invocation: ReadinessState::Ready,
            docker_mcp_bridge: ReadinessState::Ready,
            execution_target: ReadinessState::Ready,
            failure: None,
            modes: crate::harness::ExecutionMode::ALL
                .into_iter()
                .map(|mode| ModeReadinessV1 {
                    mode,
                    available: ReadinessState::Ready,
                    failure: None,
                })
                .collect(),
            roles: crate::execution::contract::HarnessRole::ALL
                .into_iter()
                .map(|role| RoleReadinessV1 {
                    role,
                    available: ReadinessState::Ready,
                    failure: None,
                })
                .collect(),
        }
    }

    fn write_settings(path: &std::path::Path, default: Option<&str>, pi_ready: bool) {
        let now = crate::persistence::harness_settings::current_unix_ms();
        let mut settings = crate::persistence::harness_settings::HarnessSettings {
            default_harness: default.map(str::to_owned),
            ..Default::default()
        };
        settings.discovered.insert(
            "pi".into(),
            DetectedHarness {
                status: "available".into(),
                version: Some("0.84.4".into()),
                executable: Some("/synthetic/pi".into()),
                diagnostic: None,
                ready: pi_ready,
                models: vec!["synthetic-default".into()],
                default_model: Some("synthetic-default".into()),
                model_catalog_fresh: true,
                configuration_required: false,
                implementation_available: true,
                checked_at_unix_ms: now,
                capability_snapshot: Some(snapshot("pi")),
            },
        );
        crate::persistence::harness_settings::with_settings_path(path.to_path_buf(), || {
            crate::persistence::harness_settings::save(&settings).unwrap();
        });
    }

    fn frame(context: &egui::Context, events: Vec<egui::Event>) -> egui::FullOutput {
        let mut output = context.run_ui(
            egui::RawInput {
                screen_rect: Some(egui::Rect::from_min_size(
                    egui::Pos2::ZERO,
                    egui::vec2(500.0, 120.0),
                )),
                events,
                ..Default::default()
            },
            super::paint,
        );
        output.textures_delta.clear();
        output
    }

    fn temp_dirs(name: &str) -> (TempDirGuard, std::path::PathBuf) {
        let dir = std::env::temp_dir().join(format!("koolade-{name}-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&dir).unwrap();
        (TempDirGuard(dir.clone()), dir.join("harness-settings.json"))
    }

    /// Removes the per-test settings root on every path, including panic
    /// (mirrors the TestTempRoot Drop idiom elsewhere in the crate).
    struct TempDirGuard(std::path::PathBuf);
    impl Drop for TempDirGuard {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

    /// Production boundary: the real router-facing settings feed the bar,
    /// and the real click path requests the Coding tools dialog through the
    /// shared context flag. Direct state tests cannot prove the visible
    /// nudge exists or is wired; no external services involved (three
    /// egui frames plus two settings round trips).
    #[test]
    fn blocked_routing_nudges_the_operator_and_one_click_requests_the_chooser() {
        let (guard, settings_path) = temp_dirs("cli-notice-click");
        write_settings(&settings_path, None, true);
        let context = egui::Context::default();
        crate::persistence::harness_settings::with_settings_path(settings_path.clone(), || {
            let output = frame(&context, vec![]);
            let center = output
                .shapes
                .iter()
                .find_map(|shape| match &shape.shape {
                    egui::Shape::Text(text) if text.galley.text().starts_with("pi is ready") => {
                        Some(text.pos + text.galley.mesh_bounds.center().to_vec2())
                    }
                    _ => None,
                })
                .expect("the ready-provider nudge is visible");
            for pressed in [true, false] {
                frame(
                    &context,
                    vec![
                        egui::Event::PointerMoved(center),
                        egui::Event::PointerButton {
                            pos: center,
                            button: egui::PointerButton::Primary,
                            pressed,
                            modifiers: Default::default(),
                        },
                    ],
                );
            }
        });
        assert_eq!(
            context.data(|data| data.get_temp::<bool>(egui::Id::new(CODING_SETTINGS_REQUEST_ID))),
            Some(true),
            "clicking the nudge must request the Coding tools dialog"
        );
        drop(guard);
    }

    /// A healthy selected CLI raises no nudge at all (negative pin: the bar
    /// must disappear once routing is satisfiable, not linger).
    #[test]
    fn healthy_selected_cli_paints_no_nudge() {
        let (guard, settings_path) = temp_dirs("cli-notice-quiet");
        write_settings(&settings_path, Some("pi"), true);
        let context = egui::Context::default();
        crate::persistence::harness_settings::with_settings_path(settings_path.clone(), || {
            let output = frame(&context, vec![]);
            assert!(
                output.shapes.iter().all(|shape| {
                    !matches!(&shape.shape, egui::Shape::Text(t) if t.galley.text().contains("coding CLI"))
                }),
                "a healthy CLI must not be nudged"
            );
        });
        drop(guard);
    }
}
