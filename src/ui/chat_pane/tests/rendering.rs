use super::support::*;
use super::*;
#[test]
fn agent_replies_paint_markdown_while_user_and_system_bubbles_stay_plain() {
    let ctx = paint_ctx();
    let messages = vec![
        ChatMessage::new(ChatRole::Agent, SHARED_MD, None),
        ChatMessage::new(ChatRole::User, SHARED_MD, None),
        ChatMessage::new(ChatRole::System, SHARED_MD, None),
    ];
    let mut draft = String::new();
    let mut output = ctx.run_ui(egui::RawInput::default(), |ui| {
        paint(ui, &messages, &mut draft, false, None, None, false);
    });
    let texts = galley_texts(&output);
    let bearing = texts.iter().filter(|t| t.contains("boldlead"));
    assert_eq!(
        bearing.count(),
        3,
        "each role paints the body once: {texts:?}"
    );
    let scrubbed = texts
        .iter()
        .filter(|t| t.contains("boldlead") && !t.contains('*'));
    assert_eq!(
        scrubbed.count(),
        1,
        "exactly the agent bubble scrubs markers: {texts:?}"
    );
    // Negative controls: User and System galleys keep every raw marker.
    for text in &texts {
        if text.contains("boldlead") && text.contains('*') {
            assert!(
                text.contains('#') && text.contains('\n'),
                "control stays verbatim: {text}"
            );
        }
    }
    // The agent bold run carries a distinct TextFormat from its sibling.
    let agent_body = galleys(&output)
        .into_iter()
        .find(|g| g.text().contains("boldlead") && !g.text().contains('*'))
        .expect("agent paragraph galley");
    let bold_section = agent_body
        .job
        .sections
        .iter()
        .find(|s| section_text(agent_body, s) == "boldlead")
        .expect("styled bold section");
    let plain_section = agent_body
        .job
        .sections
        .iter()
        .find(|s| section_text(agent_body, s) == "Lead with ")
        .expect("plain lead section");
    assert_ne!(
        bold_section.format, plain_section.format,
        "bold differs from plain"
    );
    assert!(bold_section.format.extra_letter_spacing.abs() > 1e-9);
    assert!(plain_section.format.extra_letter_spacing.abs() < f32::EPSILON);
    assert_eq!(bold_section.format.color, theme::TEXT);
    // The heading renders at CHAT.h1 in theme ink.
    let heading = galleys(&output)
        .into_iter()
        .find(|g| g.text() == "Heading")
        .expect("heading galley");
    for section in &heading.job.sections {
        assert!(
            (section.format.font_id.size - crate::ui::markdown::CHAT.h1).abs() < 1e-6,
            "heading size {}",
            section.format.font_id.size
        );
        assert_eq!(section.format.color, theme::TEXT);
    }
    output.textures_delta.clear();
}

#[test]
fn card_tab_entry_paints_the_same_agent_markdown_after_double_shield() {
    // layout.rs pre-swaps card messages' text with `readable` output
    // before paint_task re-shields; this pins that the round trip is
    // stable and the reply still renders styled and marker-free.
    let prose = "Shaped reply.\n\n- ship it\n- then **verify**";
    let raw_envelope = serde_json::json!({
        "assistant_message": prose,
        "schema_version": 1,
    })
    .to_string();
    let original = ChatMessage::new(ChatRole::Agent, raw_envelope, None);
    let pre_shielded = crate::ui::message_text::readable(&original);
    assert_eq!(pre_shielded.as_ref(), prose);
    let swapped = ChatMessage::new(ChatRole::Agent, pre_shielded.into_owned(), None);
    let re_shielded = crate::ui::message_text::readable(&swapped);
    assert_eq!(re_shielded.as_ref(), prose, "double shield must round-trip");
    let messages = vec![swapped];
    let ctx = paint_ctx();
    let mut draft = String::new();
    let mut output = ctx.run_ui(egui::RawInput::default(), |ui| {
        paint_task(ui, &messages, &mut draft, false);
    });
    let texts = galley_texts(&output);
    assert!(
        texts
            .iter()
            .any(|t| t.contains("verify") && !t.contains('*')),
        "card-tab agent reply must be marker-free: {texts:?}"
    );
    assert!(
        !texts.iter().any(|t| t.contains('{')),
        "no envelope braces: {texts:?}"
    );
    output.textures_delta.clear();
}

#[test]
fn raw_envelopes_never_reach_the_reply_galley() {
    let ctx = paint_ctx();
    let envelope =
        "{\"assistant_message\":\"Hi **you**\",\"schema_version\":1,\"open_items_updated\":[]}";
    let messages = vec![ChatMessage::new(ChatRole::Agent, envelope, None)];
    let mut draft = String::new();
    let mut output = ctx.run_ui(egui::RawInput::default(), |ui| {
        paint(ui, &messages, &mut draft, false, None, None, false);
    });
    let texts = galley_texts(&output);
    assert!(
        texts.iter().any(|t| t.contains("you") && !t.contains('*')),
        "readable prose must render styled: {texts:?}"
    );
    assert!(
        !texts
            .iter()
            .any(|t| t.contains("assistant_message") || t.contains('{')),
        "envelope leaked into a chat galley: {texts:?}"
    );
    output.textures_delta.clear();
}

#[test]
fn degenerate_agent_bodies_leave_the_pane_intact() {
    for body in ["", "   \n\t  ", "---", "solo", "```rust\nlet value = 1;"] {
        let ctx = paint_ctx();
        let messages = vec![ChatMessage::new(ChatRole::Agent, body, None)];
        let mut draft = String::new();
        let mut output = ctx.run_ui(egui::RawInput::default(), |ui| {
            paint(ui, &messages, &mut draft, false, None, None, false);
        });
        let texts = galley_texts(&output);
        // Surrounding chrome (composer hint, send affordance) survives.
        assert!(
            texts.iter().any(|t| t.contains("Ctrl + Enter to send")),
            "composer hint missing for body {body:?}"
        );
        // Painter never leaks fence glyphs, not even on an unclosed one.
        assert!(
            texts.iter().all(|t| !t.contains("```")),
            "fence glyphs leaked for body {body:?}: {texts:?}"
        );
        output.textures_delta.clear();
    }
    // The unclosed fence still lands as a monospace line in the bubble.
    let ctx = paint_ctx();
    let messages = vec![ChatMessage::new(
        ChatRole::Agent,
        "```rust\nlet value = 1;",
        None,
    )];
    let mut draft = String::new();
    let mut output = ctx.run_ui(egui::RawInput::default(), |ui| {
        paint(ui, &messages, &mut draft, false, None, None, false);
    });
    let code_line = galleys(&output)
        .into_iter()
        .find(|g| g.text() == "let value = 1;")
        .expect("unclosed fence paints its code line");
    assert!(
        code_line
            .job
            .sections
            .iter()
            .all(|s| s.format.font_id.family == egui::FontFamily::Monospace)
    );
    output.textures_delta.clear();
}
