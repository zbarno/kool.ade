use super::*;

#[test]
fn execution_telemetry_distinguishes_unmeasured_from_zero() {
    let mut output = egui::Context::default().run_ui(
        egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(
                egui::Pos2::ZERO,
                egui::vec2(640.0, 480.0),
            )),
            ..Default::default()
        },
        |ui| {
            paint_execution_telemetry(
                ui,
                &crate::persistence::execution_records::ExecutionPhaseTimingsV1::default(),
                &crate::persistence::execution_records::DockerResourceUsageV1::default(),
            );
        },
    );
    let text = rendered_text(&output.shapes);
    assert!(text.contains("Image preparation: Not recorded"));
    assert!(text.contains("Dependency broker timing is recorded per request in task activity."));
    assert!(text.contains("Docker CPU, memory, and process measurements: Not recorded"));
    output.textures_delta.clear();
}

#[test]
fn execution_telemetry_renders_measured_values_and_unavailable_fields() {
    let phases = crate::persistence::execution_records::ExecutionPhaseTimingsV1 {
        image_preparation_ms: Some(12),
        workspace_stage_in_ms: Some(7),
        workspace_stage_out_ms: Some(9),
        dependency_cache_stage_ms: Some(2),
        qa_ms: Some(25),
        cleanup_ms: Some(1),
    };
    let metrics = crate::persistence::execution_records::DockerResourceUsageV1 {
        cpu_time_ms: Some(5),
        sampled_peak_memory_bytes: Some(4096),
        latest_process_count: Some(2),
        sample_count: 3,
    };
    let mut output = egui::Context::default().run_ui(
        egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(
                egui::Pos2::ZERO,
                egui::vec2(640.0, 480.0),
            )),
            ..Default::default()
        },
        |ui| paint_execution_telemetry(ui, &phases, &metrics),
    );
    let text = rendered_text(&output.shapes);
    for expected in [
        "Image preparation: 12 ms",
        "Workspace stage in: 7 ms",
        "Workspace stage out: 9 ms",
        "Dependency cache staging: 2 ms",
        "Dependency broker timing is recorded per request in task activity.",
        "Runtime QA container command: 25 ms",
        "Container cleanup: 1 ms",
        "Container CPU since startup: 5 ms",
        "Sampled peak container memory: 4096 bytes",
        "Latest sampled process count: 2",
        "Container disk usage: Not recorded",
    ] {
        assert!(text.contains(expected), "missing {expected}: {text}");
    }
    output.textures_delta.clear();
}

fn rendered_text(shapes: &[egui::epaint::ClippedShape]) -> String {
    shapes
        .iter()
        .filter_map(|shape| match &shape.shape {
            egui::Shape::Text(text) => Some(text.galley.text().to_owned()),
            _ => None,
        })
        .collect::<Vec<_>>()
        .join("\n")
}
