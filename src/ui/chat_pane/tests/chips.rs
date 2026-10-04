use super::support::*;
use super::*;
#[test]
fn tapping_a_digest_chip_types_the_full_option_into_the_main_draft_without_sending() {
    let messages = vec![
        ChatMessage::new(ChatRole::User, "Pick a vendor.", None),
        ChatMessage::new(ChatRole::Agent, CHOICE_DIGEST, None),
    ];
    assert_eq!(crate::ui::reply_tail::open_ask_index(&messages), Some(1));
    let choices = crate::ui::reply_tail::digest_choices(&crate::ui::reply_tail::parse_reply_tail(
        CHOICE_DIGEST,
    ));
    assert_eq!(choices.len(), 2, "the two bullets offer options");
    let yes_label = crate::ui::reply_tail::choice_label(&choices[0]);
    let no_label = crate::ui::reply_tail::choice_label(&choices[1]);

    let mut run = ChipTapHarness::new(messages, false);
    let (mut output, first_intent) = run.frame(Vec::new());
    assert!(!first_intent.send, "idle frame sends nothing");
    let cells = chip_cells(&output);
    assert_eq!(cells.len(), 2, "two chips under the lifted digest");
    assert!(
        cells.iter().all(|(_, color)| *color == theme::CHIP_BORDER),
        "idle chip strokes rest on CHIP_BORDER"
    );
    // Placement: the chips follow the lifted ask row in walk order.
    let ask_spots = galley_positions(&output, "Which vendor shall we bind?");
    let yes_spots = galley_positions(&output, &yes_label);
    assert_eq!(ask_spots.len(), 1, "ask row visible");
    assert_eq!(yes_spots.len(), 1, "chip one visible");
    assert!(
        ask_spots[0] < yes_spots[0],
        "chip row rides BELOW the lifted ask"
    );
    let yes_point = galley_point(&output, &yes_label).expect("chip one centre");
    assert!(
        galley_point(&output, &no_label).is_some(),
        "chip two visible"
    );
    output.textures_delta.clear();

    assert!(
        run.ctx.memory(|mem| mem.focused()).is_none(),
        "no focus before the tap"
    );
    let (mut release_out, intent) = run.click_at(yes_point);
    release_out.textures_delta.clear();
    assert!(!intent.send, "a chip tap must NOT fire the composer");
    assert!(!intent.cancel && !intent.generate_tasks && !intent.implement_tasks);
    assert_eq!(
        run.draft, choices[0].text,
        "the FULL bullet text joins the draft, byte for byte"
    );

    // The focus request settles on the next frame; typing afterwards
    // must land behind the inserted option (caret pinned at the end).
    let (mut settle_out, _) = run.frame(Vec::new());
    settle_out.textures_delta.clear();
    assert!(
        run.ctx.memory(|mem| mem.focused()).is_some(),
        "the composer holds focus after the tap"
    );
    let (mut typed_out, typed_intent) = run.frame(vec![egui::Event::Text("x".to_string())]);
    typed_out.textures_delta.clear();
    assert!(!typed_intent.send);
    assert_eq!(
        run.draft,
        format!("{}x", choices[0].text),
        "the keystroke landed at the pinned END of the draft"
    );
}

/// AC1 negative: while the lane is busy the chips still render (dimmed,
/// stroke resting) but taps are fully inert — no draft mutation, no send.
#[test]
fn busy_panels_render_chips_but_swallow_every_tap() {
    let messages = vec![
        ChatMessage::new(ChatRole::User, "Pick a vendor.", None),
        ChatMessage::new(ChatRole::Agent, CHOICE_DIGEST, None),
    ];
    let choices = crate::ui::reply_tail::digest_choices(&crate::ui::reply_tail::parse_reply_tail(
        CHOICE_DIGEST,
    ));
    let yes_label = crate::ui::reply_tail::choice_label(&choices[0]);

    let mut run = ChipTapHarness::new(messages, true);
    let (mut output, _) = run.frame(Vec::new());
    let cells = chip_cells(&output);
    assert_eq!(
        cells.len(),
        2,
        "business dims the chips, it does not erase them"
    );
    assert!(
        cells.iter().all(|(_, color)| *color == theme::CHIP_BORDER),
        "disabled chips keep the resting border (no hover-promotion affordance)"
    );
    output.textures_delta.clear();
    let yes_point = galley_point(&output, &yes_label).expect("dimmed chip one centre");
    let (mut release_out, intent) = run.click_at(yes_point);
    release_out.textures_delta.clear();
    assert!(!intent.send, "busy tap must not send");
    assert!(run.draft.is_empty(), "busy tap must not mutate the draft");
}

/// Absence battery (AC1/AC3 discipline): answered digests, single-choice
/// digests, legacy next-step lifts and plain finals render NO chips —
/// while the still-valid lifts keep floating (backdrop survives the chip
/// gate declining). The control: an open two-choice digest renders both.
#[test]
fn chip_rows_appear_only_for_open_two_to_six_choice_digests() {
    const ONE_CHOICE: &str = "Deal?\n---\n- Yes, take it.";
    const LEGACY: &str = "SSO is recorded.\nYour next step: Should guests use SSO too?";
    let u = |text: &str| ChatMessage::new(ChatRole::User, text, None);
    let a = |text: &str| ChatMessage::new(ChatRole::Agent, text, None);
    let cases: Vec<(&str, Vec<ChatMessage>, usize)> = vec![
        (
            "answered digest",
            vec![u("Pick a vendor."), a(CHOICE_DIGEST), u("Aurora, Monday.")],
            0, // no lift at all
        ),
        (
            "single-choice digest (gate declined, lift survives)",
            vec![u("Deal?"), a(ONE_CHOICE)],
            1, // one DIGEST_BG backdrop, zero chips
        ),
        (
            "legacy next-step (lift survives, no chips)",
            vec![u("Set up SSO."), a(LEGACY)],
            1,
        ),
        (
            "marker-free plain final",
            vec![u("Status?"), a("All green, nothing blocked.")],
            0,
        ),
    ];
    for (name, messages, backdrops_expected) in cases {
        let mut run = ChipTapHarness::new(messages, false);
        let (mut output, _) = run.frame(Vec::new());
        assert_eq!(chip_cells(&output).len(), 0, "{name}: no chips at all");
        assert_eq!(
            digest_backdrops(&output),
            backdrops_expected,
            "{name}: lift backdrop expectations held"
        );
        output.textures_delta.clear();
    }
    // Control: the open two-choice digest DOES render its row.
    let mut run = ChipTapHarness::new(
        vec![
            ChatMessage::new(ChatRole::User, "Pick a vendor.", None),
            ChatMessage::new(ChatRole::Agent, CHOICE_DIGEST, None),
        ],
        false,
    );
    let (mut output, _) = run.frame(Vec::new());
    assert_eq!(
        chip_cells(&output).len(),
        2,
        "open digest renders both chips"
    );
    output.textures_delta.clear();
}
