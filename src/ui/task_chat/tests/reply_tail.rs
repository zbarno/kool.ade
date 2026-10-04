use crate::domain::{ChatMessage, ChatRole};
use crate::ui::task_chat::{Reply, split_reply, transcript};
use crate::ui::theme;
#[test]
fn adapter_maps_parsed_tails_bit_for_bit_onto_the_legacy_reply() {
    // Compatibility contract: every legacy shape must come back through
    // the shared classifier as exactly today's tuple.
    for input in [
        "SSO is recorded.\nYour next step: Should guests use SSO too?",
        "SSO is recorded.\n**Your next step:** Should guests use SSO too?",
        "Done here.\nYour next step:",
        "Your next step:",
        "SSO and MFA are confirmed.\nNo reply needed.",
        "Anything else?",
        "Patch landed.\nDeploy tonight?",
        "Quiet workday.",
    ] {
        let tail = crate::ui::reply_tail::parse_reply_tail(input);
        assert_eq!(
            split_reply(input),
            Reply {
                summary: tail.body,
                next: tail.ask,
                no_reply: tail.no_reply,
            },
            "legacy parity drifted for {input:?}"
        );
    }
    // The unreleased digest shape maps body → summary and the first
    // bullet → next, landing 1:1 on the card 'Your answer needed' line.
    let input =
        "Draft ready.\n\n---\n- Enable SSO for all guests?\n- Recommended: yes, effective Monday.";
    let tail = crate::ui::reply_tail::parse_reply_tail(input);
    assert_eq!(tail.kind, crate::ui::reply_tail::TailKind::Digest);
    let reply = split_reply(input);
    assert_eq!(
        reply,
        Reply {
            summary: tail.body.clone(),
            next: tail.ask.clone(),
            no_reply: tail.no_reply,
        }
    );
    assert_eq!(reply.summary, "Draft ready.");
    assert_eq!(reply.next.as_deref(), Some(tail.bullets[0].as_str()));
    assert_eq!(reply.next.as_deref(), Some("Enable SSO for all guests?"));
    assert!(!reply.no_reply);
}

/// Transcript lift (test plan 4b): a legacy 'Your next step:' final reply
/// floats its question verbatim and strongly inside the card transcript,
/// leaves no marker in any galley, and retires the moment a user answer
/// lands.
#[test]
fn transcript_lifts_legacy_asks_verbatim_and_retires_them_on_answer() {
    const LEGACY: &str = "SSO is recorded.\nYour next step: Should guests use SSO too?";
    const QUESTION: &str = "Should guests use SSO too?";
    let ctx = egui::Context::default();
    ctx.set_visuals(theme::koolade_visuals());

    fn backdrops(output: &egui::FullOutput) -> usize {
        output
            .shapes
            .iter()
            .filter(|clipped| match &clipped.shape {
                egui::Shape::Rect(rect) => rect.fill == theme::DIGEST_BG,
                _ => false,
            })
            .count()
    }
    fn texts_of(output: &egui::FullOutput) -> Vec<String> {
        output
            .shapes
            .iter()
            .filter_map(|clipped| match &clipped.shape {
                egui::Shape::Text(shape) => Some((*shape.galley).text().to_owned()),
                _ => None,
            })
            .collect()
    }
    fn strong_spot(output: &egui::FullOutput, needle: &str) -> Option<bool> {
        output
            .shapes
            .iter()
            .filter_map(|clipped| match &clipped.shape {
                egui::Shape::Text(shape) => Some(&*shape.galley),
                _ => None,
            })
            .find(|g| g.text() == needle)
            .map(|g| {
                let section = g
                    .job
                    .sections
                    .iter()
                    .find(|s| s.byte_range.end.0 > s.byte_range.start.0)
                    .expect("non-empty section");
                section.format.extra_letter_spacing.abs() > 1e-9
            })
    }

    let user = ChatMessage::new(ChatRole::User, "Set up SSO.", None);
    let agent = ChatMessage::new(ChatRole::Agent, LEGACY, None);

    // Open: the question floats verbatim, unlabeled and strong.
    let open_log = vec![user.clone(), agent.clone()];
    let mut output = ctx.run_ui(egui::RawInput::default(), |ui| {
        egui::CentralPanel::default().show(ui, |ui| transcript(ui, &open_log, false));
    });
    assert_eq!(
        backdrops(&output),
        1,
        "legacy ask must float in the transcript"
    );
    let texts = texts_of(&output);
    assert_eq!(
        texts.iter().filter(|t| *t == QUESTION).count(),
        1,
        "question lifts verbatim exactly once: {texts:?}"
    );
    assert!(
        !texts.iter().any(|t| t.contains("Your next step:")),
        "marker leaked: {texts:?}"
    );
    assert!(
        texts.iter().any(|t| t.contains("SSO is recorded.")),
        "transcript body must still render: {texts:?}"
    );
    assert!(
        strong_spot(&output, QUESTION).unwrap_or(false),
        "lifted row prints strongly"
    );
    output.textures_delta.clear();

    // Answered: the lift disappears entirely.
    let answered_log = vec![
        user,
        agent,
        ChatMessage::new(ChatRole::User, "Yes, guests included.", None),
    ];
    let mut after = ctx.run_ui(egui::RawInput::default(), |ui| {
        egui::CentralPanel::default().show(ui, |ui| transcript(ui, &answered_log, false));
    });
    assert_eq!(backdrops(&after), 0, "a user answer must retire the lift");
    let after_texts = texts_of(&after);
    assert_eq!(
        after_texts.iter().filter(|t| *t == QUESTION).count(),
        0,
        "no lifted row may survive the answer: {after_texts:?}"
    );
    after.textures_delta.clear();
}
