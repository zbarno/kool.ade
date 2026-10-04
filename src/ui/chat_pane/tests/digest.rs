use super::support::*;
use super::*;
#[test]
fn digest_reply_floats_an_unlabeled_lift_directly_under_the_main_chat_body() {
    let messages = vec![
        ChatMessage::new(ChatRole::User, "Bind the vendor please.", None),
        ChatMessage::new(ChatRole::Agent, DIGEST_FIXTURE, None),
    ];
    assert_eq!(
        crate::ui::reply_tail::open_ask_index(&messages),
        Some(1),
        "the digest reply must select as the open ask"
    );
    let ctx = paint_ctx();
    let mut draft = String::new();
    let mut output = ctx.run_ui(egui::RawInput::default(), |ui| {
        paint(ui, &messages, &mut draft, false, None, None, false);
    });
    // (a) Exactly one lifted backdrop.
    assert_eq!(
        digest_backdrops(&output),
        1,
        "exactly one DIGEST_BG backdrop expected"
    );
    // (b) Hairline bookkeeping: pane chrome + body rules + the ONE lift
    // rule. The lifted body no longer carries its '---', so the body
    // reference contributes zero here.
    let expected_lines = main_chrome_border_lines() + body_border_lines(&messages, Some(1)) + 1;
    assert_eq!(
        border_lines(&output),
        expected_lines,
        "lift must add exactly one BORDER hairline"
    );
    let texts = galley_texts(&output);
    // The Markdown body renders only the pre-tail prose.
    assert!(
        texts.iter().any(|t| t == "Draft ready."),
        "trimmed body must render: {texts:?}"
    );
    // (c) Every row floats exactly once, in payload order.
    assert_rows_in_payload_order(&output, &DIGEST_ROWS);
    // (d) The lead row carries a stronger TextFormat than the second.
    let lead = galleys(&output)
        .into_iter()
        .find(|g| g.text() == DIGEST_ROWS[0])
        .expect("lead row galley");
    let second = galleys(&output)
        .into_iter()
        .find(|g| g.text() == DIGEST_ROWS[1])
        .expect("second row galley");
    let (lead_fmt, second_fmt) = (lead_section_format(lead), lead_section_format(second));
    assert_ne!(lead_fmt, second_fmt, "lead row must be stronger");
    assert!(
        lead_fmt.extra_letter_spacing.abs() > 1e-9,
        "lead row uses the tracked-bold convention (got {})",
        lead_fmt.extra_letter_spacing
    );
    assert!(
        second_fmt.extra_letter_spacing.abs() < f32::EPSILON,
        "second row stays regular"
    );
    // (e) Purity: no rule glyphs, no bare list markers, no invented label
    // words. (The rows are asserted byte-equal to the parsed payload
    // above, which is the machine check that the painter printed nothing
    // besides the ask/recommendation/pointer text itself.)
    assert!(
        !texts.iter().any(|t| t.contains("---")),
        "rule glyphs leaked: {texts:?}"
    );
    assert!(
        !texts.iter().any(|t| t.trim_start().starts_with("- ")),
        "bare list markers leaked: {texts:?}"
    );
    assert!(
        !texts
            .iter()
            .any(|t| t.starts_with("Ask:") || t.starts_with("Recommendation:")),
        "the lift must stay unlabeled: {texts:?}"
    );
    output.textures_delta.clear();
}

/// AC 2: the instant a user answer follows the digest, zero backdrops and
/// no lift hairline — only the body-intrinsic rule of the still-printed
/// reply text may remain.
#[test]
fn a_user_answer_immediately_retires_the_lift() {
    let messages = vec![
        ChatMessage::new(ChatRole::User, "Bind the vendor please.", None),
        ChatMessage::new(ChatRole::Agent, DIGEST_FIXTURE, None),
        ChatMessage::new(ChatRole::User, "Go with Aurora.", None),
    ];
    assert_eq!(
        crate::ui::reply_tail::open_ask_index(&messages),
        None,
        "an answered ask must deselect"
    );
    let ctx = paint_ctx();
    let mut draft = String::new();
    let mut output = ctx.run_ui(egui::RawInput::default(), |ui| {
        paint(ui, &messages, &mut draft, false, None, None, false);
    });
    assert_eq!(
        digest_backdrops(&output),
        0,
        "user answer must retire the backdrop"
    );
    let expected_lines = main_chrome_border_lines() + body_border_lines(&messages, None);
    assert_eq!(
        border_lines(&output),
        expected_lines,
        "no lift hairline may survive an answer (allowed: {})",
        expected_lines
    );
    output.textures_delta.clear();
}

/// AC 3: the legacy 'Your next step:' reply lifts on Main Chat for the
/// first time — the question floats strongly and verbatim, the marker is
/// gone from every galley, and the body still renders.
#[test]
fn legacy_your_next_step_lifts_on_main_chat_with_no_marker_leak() {
    const LEGACY: &str = "SSO is recorded.\nYour next step: Should guests use SSO too?";
    const QUESTION: &str = "Should guests use SSO too?";
    let messages = vec![
        ChatMessage::new(ChatRole::User, "Set up SSO.", None),
        ChatMessage::new(ChatRole::Agent, LEGACY, None),
    ];
    assert_eq!(crate::ui::reply_tail::open_ask_index(&messages), Some(1));
    let ctx = paint_ctx();
    let mut draft = String::new();
    let mut output = ctx.run_ui(egui::RawInput::default(), |ui| {
        paint(ui, &messages, &mut draft, false, None, None, false);
    });
    assert_eq!(digest_backdrops(&output), 1, "legacy ask must float");
    let texts = galley_texts(&output);
    assert_eq!(
        galley_positions(&output, QUESTION).len(),
        1,
        "question lifts verbatim exactly once: {texts:?}"
    );
    let lifted = galleys(&output)
        .into_iter()
        .find(|g| g.text() == QUESTION)
        .expect("lifted question galley");
    assert!(
        lead_section_format(lifted).extra_letter_spacing.abs() > 1e-9,
        "lifted legacy ask must print strongly"
    );
    assert!(
        !texts.iter().any(|t| t.contains("Your next step:")),
        "marker leaked: {texts:?}"
    );
    assert!(
        texts.iter().any(|t| t.contains("SSO is recorded.")),
        "Markdown body must still render: {texts:?}"
    );
    assert_eq!(
        border_lines(&output),
        main_chrome_border_lines() + body_border_lines(&messages, Some(1)) + 1,
        "one lift hairline for the legacy ask"
    );
    output.textures_delta.clear();
}
