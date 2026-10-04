use super::support::*;
use super::*;
#[test]
fn live_stream_keeps_diagnostic_regimes_and_formats_prose_chunks() {
    let streaming_md = "Streamed **boldchunk** and a half fence\n```rust\nlet n = 2;";
    let progress = LiveProgress {
        posts: vec![
            LivePost {
                id: (1, 0),
                kind: "thinking".to_string(),
                text: "reasoning **still plain**".to_string(),
            },
            LivePost {
                id: (2, 0),
                kind: "tool".to_string(),
                text: "cargo build\nCompiling koolade v0.1.0".to_string(),
            },
            LivePost {
                id: (3, 0),
                kind: "text".to_string(),
                text: streaming_md.to_string(),
            },
        ],
        ..Default::default()
    };
    let ctx = paint_ctx();
    // Frame 1: the thinking diagnostic is open by default and stays
    // plainly labeled; the streamed prose renders through the painter.
    let mut first = ctx.run_ui(egui::RawInput::default(), |ui| {
        paint_progress(ui, &progress);
    });
    let texts = galley_texts(&first);
    assert!(
        texts
            .iter()
            .any(|t| t.contains("boldchunk") && !t.contains('*')),
        "streamed prose must be marker-free: {texts:?}"
    );
    assert!(
        texts
            .iter()
            .any(|t| t.contains("reasoning **still plain**")),
        "plain thinking body must show its literal markers: {texts:?}"
    );
    // Locate the tool diagnostics header and click it open.
    let header_center = first
        .shapes
        .iter()
        .filter_map(|clipped| match &clipped.shape {
            egui::Shape::Text(shape) => Some(shape),
            _ => None,
        })
        .find(|shape| (*shape.galley).text().starts_with("Tool output"))
        .map(|shape| shape.pos + (*shape.galley).size() * 0.5)
        .expect("tool diagnostics header");
    let click = egui::RawInput {
        events: vec![
            egui::Event::PointerMoved(header_center),
            egui::Event::PointerButton {
                pos: header_center,
                button: egui::PointerButton::Primary,
                pressed: true,
                modifiers: Default::default(),
            },
            egui::Event::PointerButton {
                pos: header_center,
                button: egui::PointerButton::Primary,
                pressed: false,
                modifiers: Default::default(),
            },
        ],
        ..Default::default()
    };
    // The collapsible fades open over a short wall-clock animation, so
    // keep painting frames until the tool body is fully disclosed.
    let mut saw_tool_mono = false;
    for _ in 0..120 {
        let input = if !saw_tool_mono {
            click.clone()
        } else {
            egui::RawInput::default()
        };
        let mut out = ctx.run_ui(input, |ui| paint_progress(ui, &progress));
        for galley in galleys(&out) {
            if galley.text() == "cargo build\nCompiling koolade v0.1.0" {
                saw_tool_mono = galley
                    .job
                    .sections
                    .iter()
                    .all(|sect| sect.format.font_id.family == egui::FontFamily::Monospace);
                break;
            }
        }
        out.textures_delta.clear();
        if saw_tool_mono {
            break;
        }
        std::thread::sleep(std::time::Duration::from_millis(2));
    }
    assert!(saw_tool_mono, "disclosed tool output must stay monospace");
    first.textures_delta.clear();

    // Posts empty: the response fallback renders through the painter.
    let response_only = LiveProgress {
        response: "Final **word** lands.".to_string(),
        ..Default::default()
    };
    let mut output = ctx.run_ui(egui::RawInput::default(), |ui| {
        paint_progress(ui, &response_only);
    });
    let texts = galley_texts(&output);
    assert!(
        texts.iter().any(|t| t.contains("word") && !t.contains('*')),
        "fallback response must be marker-free: {texts:?}"
    );
    output.textures_delta.clear();
}

// ------------------------------------------------------------------
