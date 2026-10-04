use crate::domain::{ChatMessage, ChatRole};
use crate::ui::task_chat::transcript;
use crate::ui::theme;
#[test]
fn transcript_roles_gate_markdown_like_the_main_pane() {
    const MD: &str = "Card says **boldcard** now.";
    let messages = vec![
        ChatMessage::new(ChatRole::Agent, MD, None),
        ChatMessage::new(ChatRole::User, MD, None),
        ChatMessage::new(ChatRole::System, MD, None),
    ];
    let ctx = egui::Context::default();
    ctx.set_visuals(theme::koolade_visuals());
    let mut output = ctx.run_ui(egui::RawInput::default(), |ui| {
        egui::CentralPanel::default().show(ui, |ui| transcript(ui, &messages, false));
    });
    let texts: Vec<String> = output
        .shapes
        .iter()
        .filter_map(|clipped| match &clipped.shape {
            egui::Shape::Text(shape) => Some((*shape.galley).text().to_owned()),
            _ => None,
        })
        .collect();
    let bearing = texts.iter().filter(|t| t.contains("boldcard")).count();
    assert_eq!(bearing, 3, "each role paints its body once: {texts:?}");
    let scrubbed = texts
        .iter()
        .filter(|t| t.contains("boldcard") && !t.contains('*'))
        .count();
    assert_eq!(
        scrubbed, 1,
        "only the agent entry renders Markdown: {texts:?}"
    );
    output.textures_delta.clear();
}
