use super::support::*;
use super::*;
/// AC 4 negative battery: NoReply finals, marker-free plain finals,
/// answered digests and six-bullet overloads all paint the plain body
/// with ABSOLUTELY no tail chrome.
#[test]
fn non_asking_finals_paint_the_plain_body_with_zero_tail_chrome() {
    const OVERLOAD: &str = "Deck.\n---\n- One\n- Two\n- Three\n- Four\n- Five\n- Six";
    let u = |text: &str| ChatMessage::new(ChatRole::User, text, None);
    let a = |text: &str| ChatMessage::new(ChatRole::Agent, text, None);
    let cases: Vec<(&str, Vec<ChatMessage>)> = vec![
        (
            "NoReply-final",
            vec![
                u("Confirm SSO."),
                a("SSO and MFA are confirmed.\nNo reply needed."),
            ],
        ),
        (
            "marker-free plain final",
            vec![u("Status?"), a("All green, nothing blocked.")],
        ),
        (
            "digest-then-user",
            vec![
                u("Bind the vendor please."),
                a(DIGEST_FIXTURE),
                u("Go with Aurora."),
            ],
        ),
        (
            "six-bullet overload degrades to Plain",
            vec![u("Deal breaker?"), a(OVERLOAD)],
        ),
    ];
    let chrome = main_chrome_border_lines();
    for (name, messages) in cases {
        assert_eq!(
            crate::ui::reply_tail::open_ask_index(&messages),
            None,
            "{name}: must not select"
        );
        let ctx = paint_ctx();
        let mut draft = String::new();
        let mut output = ctx.run_ui(egui::RawInput::default(), |ui| {
            paint(ui, &messages, &mut draft, false, None, None, false);
        });
        assert_eq!(
            digest_backdrops(&output),
            0,
            "{name}: zero backdrops expected"
        );
        let allowed = chrome + body_border_lines(&messages, None);
        assert_eq!(
            border_lines(&output),
            allowed,
            "{name}: no lift hairline (allowed body/chrome lines: {allowed})"
        );
        output.textures_delta.clear();
    }
}

/// AC 6: an intentionally broken envelope shields to plain prose and can
/// never feed the lift — no braces or envelope keys may reach a galley.
#[test]
fn broken_envelopes_shield_to_prose_and_can_never_feed_the_lift() {
    const BROKEN: &str = "{\"assistant_message\":\"Draft ready.\\n\\n---\\n- Which vendor shal";
    let messages = vec![
        ChatMessage::new(ChatRole::User, "Vendor question pending.", None),
        ChatMessage::new(ChatRole::Agent, BROKEN, None),
    ];
    let ctx = paint_ctx();
    let mut draft = String::new();
    let mut output = ctx.run_ui(egui::RawInput::default(), |ui| {
        paint(ui, &messages, &mut draft, false, None, None, false);
    });
    assert_eq!(digest_backdrops(&output), 0, "shield note must not lift");
    assert_eq!(
        border_lines(&output),
        main_chrome_border_lines(),
        "no hairline of any kind may come from the shield path"
    );
    let texts = galley_texts(&output);
    assert!(
        texts.iter().any(|t| t.contains("unreadable reply")),
        "shield note must render as plain prose: {texts:?}"
    );
    assert!(
        !texts.iter().any(|t| t.contains('{') || t.contains('}')),
        "braces leaked into a galley: {texts:?}"
    );
    assert!(
        !texts.iter().any(|t| t.contains("assistant_message")),
        "envelope key leaked: {texts:?}"
    );
    output.textures_delta.clear();
}

/// AC 5: the card tab paints the identical lift at Main Chat parity —
/// same backdrop, same hairline, same rows in the same order.
#[test]
fn card_tabs_paint_the_identical_lift_as_main_chat() {
    let messages = vec![
        ChatMessage::new(ChatRole::User, "Bind the vendor please.", None),
        ChatMessage::new(ChatRole::Agent, DIGEST_FIXTURE, None),
    ];
    assert_eq!(crate::ui::reply_tail::open_ask_index(&messages), Some(1));
    let ctx = paint_ctx();
    let mut draft = String::new();
    let mut main_out = ctx.run_ui(egui::RawInput::default(), |ui| {
        paint(ui, &messages, &mut draft, false, None, None, false);
    });
    let ctx = paint_ctx();
    let mut draft = String::new();
    let mut tab_out = ctx.run_ui(egui::RawInput::default(), |ui| {
        paint_task(ui, &messages, &mut draft, false);
    });
    // Identical findings on both surfaces.
    assert_eq!(
        digest_backdrops(&tab_out),
        1,
        "card tab must float the lift"
    );
    assert_eq!(
        digest_backdrops(&tab_out),
        digest_backdrops(&main_out),
        "backdrop parity"
    );
    assert_rows_in_payload_order(&tab_out, &DIGEST_ROWS);
    assert_rows_in_payload_order(&main_out, &DIGEST_ROWS);
    // Tab hairline bookkeeping: tab chrome + lifted body rules + ONE rule.
    assert_eq!(
        border_lines(&tab_out),
        card_tab_chrome_border_lines() + body_border_lines(&messages, Some(1)) + 1,
        "card tab must add exactly one lift hairline"
    );
    assert!(border_lines(&main_out) >= 1, "main chat hairline present");
    main_out.textures_delta.clear();
    tab_out.textures_delta.clear();
}

// ------------------------------------------------------------------
