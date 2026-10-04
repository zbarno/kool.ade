/// AC1/AC3: the compact answer-needed card keeps its heading, action and
/// Send-answer affordance while choice chips sit between the action and
/// composer; tapping a chip joins its full text without sending.
use super::card_support::*;
use crate::domain::{ChatMessage, ChatRole};
use crate::ui::theme;
#[test]
fn collapsed_card_nested_chips_spacejoin_the_tapped_option_into_the_task_draft() {
    let choices = crate::ui::reply_tail::digest_choices(&crate::ui::reply_tail::parse_reply_tail(
        CARD_DIGEST,
    ));
    assert_eq!(choices.len(), 2, "the two bullets offer options");
    let yes_label = crate::ui::reply_tail::choice_label(&choices[0]);
    let no_label = crate::ui::reply_tail::choice_label(&choices[1]);

    let ctx = egui::Context::default();
    ctx.set_visuals(theme::koolade_visuals());
    let mut probe = CardProbe::answer_card(
        "Binding vendor for the checkout rollout", // ≠ action → action label paints
        CARD_DIGEST,
    );
    probe.draft = "Short answer:".to_string();

    let mut out = card_frame(&ctx, &mut probe, false, Vec::new());
    let heading_spot =
        card_walk_index(&out, "Your answer needed").expect("the answer-needed heading survives");
    let action_spot =
        card_walk_index(&out, "Which vendor shall we bind?").expect("the action row survives");
    let send_spot = card_walk_index(&out, "Send answer").expect("the send affordance survives");
    let yes_spot = card_walk_index(&out, &yes_label).expect("chip one projects");
    let no_spot = card_walk_index(&out, &no_label).expect("chip two projects");
    // Nesting: action, THEN chips, THEN the composer/send.
    assert!(
        action_spot < yes_spot && yes_spot < no_spot && no_spot < send_spot,
        "chips interleave action → chips → composer ({} {} {})",
        action_spot,
        yes_spot,
        send_spot
    );
    assert!(heading_spot < action_spot, "heading leads the frame");
    let cells = card_chip_cells(&out);
    assert_eq!(cells.len(), 2, "radius-10 CHIP_FILL cells, one per choice");
    assert!(
        cells.iter().all(|(_, color)| *color == theme::CHIP_BORDER),
        "resting strokes"
    );
    let no_point = card_point(&out, &no_label).expect("chip two centre");
    out.textures_delta.clear();

    // Gesture: move + press, then release — mirroring the pane e2e.
    card_frame(&ctx, &mut probe, false, press_events(no_point))
        .textures_delta
        .clear();
    card_frame(&ctx, &mut probe, false, release_events(no_point))
        .textures_delta
        .clear();

    assert_eq!(
        probe.sent, 0,
        "a chip tap must never invoke send_task_reply"
    );
    assert_eq!(probe.messages.len(), 2, "no reply began on the task lane");
    assert_eq!(
        probe.draft, "Short answer: No, keep Postman.",
        "compact card space-joins the FULL chosen text"
    );

    // DoD caret proof on the card composer: the focus request settles on
    // the next frame, and a keystroke afterwards must LAND BEHIND the
    // inserted option (caret pinned at the drafted end, not a stale
    // midpoint).
    card_frame(&ctx, &mut probe, false, Vec::new())
        .textures_delta
        .clear();
    card_frame(
        &ctx,
        &mut probe,
        false,
        vec![egui::Event::Text("x".to_string())],
    )
    .textures_delta
    .clear();
    assert_eq!(
        probe.draft, "Short answer: No, keep Postman.x",
        "the post-tap keystroke landed at the pinned END of the draft"
    );
}

/// AC1 expanded mode: the multiline task composer receives a NEWLINE join
/// for the tapped choice (matching the existing task-chat join convention
/// the ticket prescribes for expanded drafts).
#[test]
fn expanded_card_nested_chips_newlinejoin_the_tapped_option() {
    let choices = crate::ui::reply_tail::digest_choices(&crate::ui::reply_tail::parse_reply_tail(
        CARD_DIGEST,
    ));
    assert_eq!(choices.len(), 2);
    let yes_label = crate::ui::reply_tail::choice_label(&choices[0]);

    let ctx = egui::Context::default();
    ctx.set_visuals(theme::koolade_visuals());
    let mut probe = CardProbe::answer_card("Binding vendor for the checkout rollout", CARD_DIGEST);
    probe.draft = "Long rationale:".to_string();

    let mut out = card_frame(&ctx, &mut probe, true, Vec::new());
    assert!(
        card_walk_index(&out, "Your answer needed").is_some(),
        "expanded card keeps the heading"
    );
    let cells = card_chip_cells(&out);
    assert_eq!(cells.len(), 2, "expanded frame nests the chips too");
    let yes_point = card_point(&out, &yes_label).expect("chip one centre");
    out.textures_delta.clear();

    card_frame(&ctx, &mut probe, true, press_events(yes_point))
        .textures_delta
        .clear();
    card_frame(&ctx, &mut probe, true, release_events(yes_point))
        .textures_delta
        .clear();

    assert_eq!(probe.sent, 0, "expanded tap must not send either");
    assert_eq!(
        probe.draft, "Long rationale:\nYes, bind Aurora effective Monday.",
        "expanded card newline-joins the FULL chosen text"
    );
}

/// AC1 negative: a busy task lane DIMS the row (still two radius-10
/// cells) and swallows the tap — no draft mutation, no send attempt.
#[test]
fn busy_cards_dim_the_chips_and_swallow_taps() {
    let choices = crate::ui::reply_tail::digest_choices(&crate::ui::reply_tail::parse_reply_tail(
        CARD_DIGEST,
    ));
    let no_label = crate::ui::reply_tail::choice_label(&choices[1]);

    let ctx = egui::Context::default();
    ctx.set_visuals(theme::koolade_visuals());
    let mut probe = CardProbe::answer_card("Binding vendor for the checkout rollout", CARD_DIGEST);
    probe.busy = true;
    probe.draft = "Kept.".to_string();

    let mut out = card_frame(&ctx, &mut probe, false, Vec::new());
    let cells = card_chip_cells(&out);
    assert_eq!(
        cells.len(),
        2,
        "busyness dims, it does not remove, the chips"
    );
    assert!(
        cells.iter().all(|(_, color)| *color == theme::CHIP_BORDER),
        "no hover promotion while busy"
    );
    let no_point = card_point(&out, &no_label).expect("dimmed chip centre");
    out.textures_delta.clear();

    card_frame(&ctx, &mut probe, false, press_events(no_point))
        .textures_delta
        .clear();
    card_frame(&ctx, &mut probe, false, release_events(no_point))
        .textures_delta
        .clear();

    assert_eq!(
        probe.draft, "Kept.",
        "busy tap must leave the draft untouched"
    );
    assert_eq!(probe.sent, 0, "busy tap must not send");
}

/// AC1/AC3 discipline on the card surface: a legacy next-step final, a
/// single-choice digest, an answered digest and a plain final each paint
/// ZERO chips even though the frame itself still behaves (heading
/// intact); the control open two-choice digest renders both.
#[test]
fn cards_without_an_open_two_to_six_choice_digest_offer_no_chips() {
    let u = |text: &str| ChatMessage::new(ChatRole::User, text, None);
    let a = |text: &str| ChatMessage::new(ChatRole::Agent, text, None);
    let cases: Vec<(&str, Vec<ChatMessage>)> = vec![
        (
            "legacy next-step final",
            vec![
                u("Set up SSO."),
                a("SSO is recorded.\nYour next step: Should guests use SSO too?"),
            ],
        ),
        (
            "single-choice digest",
            vec![u("Deal?"), a("Deal?\n---\n- Yes, take it.")],
        ),
        (
            "answered digest (retry frame)",
            vec![u("Pick a vendor."), a(CARD_DIGEST), u("Aurora, Monday.")],
        ),
        (
            "marker-free plain final",
            vec![u("Status?"), a("All green, nothing blocked.")],
        ),
    ];
    for (name, messages) in cases {
        let ctx = egui::Context::default();
        ctx.set_visuals(theme::koolade_visuals());
        let mut probe =
            CardProbe::answer_card("Binding vendor for the checkout rollout", CARD_DIGEST);
        probe.messages = messages;
        let mut out = card_frame(&ctx, &mut probe, false, Vec::new());
        assert_eq!(card_chip_cells(&out).len(), 0, "{name}: zero chips");
        out.textures_delta.clear();
    }
    // Control: the open two-choice digest offers its row.
    let ctx = egui::Context::default();
    ctx.set_visuals(theme::koolade_visuals());
    let mut probe = CardProbe::answer_card("Binding vendor for the checkout rollout", CARD_DIGEST);
    let mut out = card_frame(&ctx, &mut probe, false, Vec::new());
    assert_eq!(
        card_chip_cells(&out).len(),
        2,
        "open digest renders both cards' chips"
    );
    out.textures_delta.clear();
}
