use super::*;

#[test]
fn task_mode_instructions_generalize_to_the_digest_contract() {
    // Scoped task-mode turn: the generalized tail contract must ride in.
    let (mut inputs, dir_task) = inputs_for("digest_taskcap", "Which SSO route?");
    inputs.state.items.push(crate::domain::OpenItem::new(
        "CLR-001".into(),
        crate::domain::Priority::Normal,
        crate::domain::ItemKind::Question,
        "Engineering".into(),
        None,
        "Choose the SSO route.".into(),
        "Blocks the rollout draft.".into(),
    ));
    let key = inputs
        .state
        .items
        .last()
        .expect("CLR-001 pushed")
        .conversation_key()
        .to_string();
    let sink: Arc<std::sync::Mutex<Option<String>>> = Arc::new(std::sync::Mutex::new(None));
    let c = TurnController::start_scoped(
        inputs,
        Box::new(CaptureHarness {
            raw: canned_raw("Routes compared; a choice is needed before rollout."),
            sink: Arc::clone(&sink),
        }),
        Some(key),
    );
    assert!(matches!(drain(&c), TurnOutcome::Applied { .. }));
    let task_instr = sink
        .lock()
        .unwrap()
        .take()
        .expect("scoped turn captured its instructions");
    assert!(
        task_instr.contains("TASK CONVERSATION MODE"),
        "task-mode banner missing from the scoped instructions"
    );
    for needle in [
        "excluding any reply-tail digest",
        "unlabeled reply-tail digest",
        "one line containing only ---,",
        "ordered ask, recommendation, pointer",
        "'Option 1', 'Option 2',",
        // The closeout phrase keeps its exactly-once occurrence in
        // this source file (the contract line itself), so this pin is
        // assembled from two adjacent literals.
        concat!("end with 'No reply ", "needed.' and no digest"),
    ] {
        assert!(
            task_instr.contains(needle),
            "task instructions missing: {needle:?}"
        );
    }
    // The retired line-emission order is provably gone. Its spelling is
    // split so the retired marker survives in no source line of this
    // file (the definition of done greps the retired lead-in away from
    // src/core entirely).
    let retired_line_order = concat!("beginning ", "exactly 'Your next step:'");
    assert!(
        !task_instr.contains(retired_line_order),
        "retired line-emission order survived in the task prose"
    );

    // Task-less twin: the main-chat path injects the standing digest
    // paragraph and nothing task-shaped.
    let (inputs, dir_main) = inputs_for("digest_maincap", "Which SSO route? (main chat)");
    let main_sink: Arc<std::sync::Mutex<Option<String>>> = Arc::new(std::sync::Mutex::new(None));
    let c = TurnController::start(
        inputs,
        Box::new(CaptureHarness {
            raw: canned_raw("Routes compared; a choice is needed before rollout."),
            sink: Arc::clone(&main_sink),
        }),
    );
    assert!(matches!(drain(&c), TurnOutcome::Applied { .. }));
    let main_instr = main_sink
        .lock()
        .unwrap()
        .take()
        .expect("main-chat turn captured its instructions");
    assert!(
        main_instr.contains(concat!("REPLY-TAIL ", "DIGEST (display convention")),
        "main-chat path missing the standing digest contract"
    );
    assert!(
        !main_instr.contains("TASK CONVERSATION MODE"),
        "task-mode banner must not leak into main chat"
    );
    assert!(
        !main_instr.contains("Your next step"),
        "legacy tail grammar must be absent from the main-chat contract"
    );
    let _ = std::fs::remove_dir_all(&dir_task);
    let _ = std::fs::remove_dir_all(&dir_main);
}

#[test]
fn digest_does_not_disturb_envelope_extraction() {
    use crate::harness::pi_extract::extract_json_object;
    use crate::ui::reply_tail::{TailKind, parse_reply_tail};

    // Canonical envelope object; its compact serialization is exactly
    // what rides inside each fenced block below.
    let env_value = serde_json::json!({
        "schema_version": 1,
        "assistant_message": "Choices drafted; pick one before Friday.",
        "change_summary": "note the SSO choice point",
        "open_items_added": [],
        "open_items_updated": [],
        "open_items_resolved": []
    });
    let env_json = env_value.to_string();

    // with = body + digest + trailing fence; minus = body + trailing
    // fence — identical leading body bytes, identical trailing bytes.
    let wrap = |body: &str, digest: &str| format!("{body}{digest}\n\n```json\n{env_json}\n```");

    // A: five-bullet digest with labeled option bullets.
    let body_a = "We have a route-by-route comparison for the SSO rollout.";
    let digest_a = "\n---\n- Adopt one SSO route before Friday?\n- Recommend Option 1: least client work.\n- Comparison on the SSO task card.\n- Option 1: IdP-managed sessions.\n- Option 2: Server-held refresh tokens.";
    // B: single-bullet digest.
    let body_b = "The deploy checklist is complete.";
    let digest_b = "\n---\n- Confirm the Monday deploy window.";
    // C: adversarial — an EARLIER balanced draft fence inside the body
    // that a greedy extractor could mistake for the envelope.
    let body_c = "Draft noise first, real envelope later.\n\n```json\n{\"schema_version\": 1, \"assistant_message\": \"wrong draft\", \"change_summary\": \"noop\"}\n```\n\nRechecking the shape.";
    let digest_c =
        "\n---\n- Keep the drafting fence in prose?\n- Pointer: the extraction notes below.";
    // D: adversarial — the DIGEST ITSELF embeds brace clusters and a
    // fence-opening token mid-prose; the scanner must sail straight
    // past them and land on the real trailing fence.
    let body_d = "The flag plan is settled except for one yes-or-no.";
    let digest_d = "\n---\n- Ship behind release_gate v2?\n- Payload shape stays {\"flag\":true} untouched.\n- Token sample ```json {\"probe\":1} is inert here.\n- Pointer: the parity appendix.";

    for (label, body, digest) in [
        ("A five-bullet option digest", body_a, digest_a),
        ("B single-bullet digest", body_b, digest_b),
        ("C earlier draft fence", body_c, digest_c),
        ("D brace-laden digest with fence token", body_d, digest_d),
    ] {
        let with_digest = wrap(body, digest);
        let minus_digest = wrap(body, "");
        let with_blob = extract_json_object(&with_digest);
        let minus_blob = extract_json_object(&minus_digest);
        assert!(
            with_blob.is_some() && minus_blob.is_some(),
            "{label}: envelope missing on one side"
        );
        assert_eq!(
            with_blob.as_deref(),
            minus_blob.as_deref(),
            "{label}: the digest displaced the extracted envelope bytes"
        );
        // ...and what was extracted is the canonical envelope, proving
        // no earlier fence or digest text won.
        let parsed: serde_json::Value =
            serde_json::from_str(with_blob.as_deref().unwrap()).unwrap();
        assert_eq!(
            parsed, env_value,
            "{label}: extraction is not the canonical envelope"
        );

        // decode_envelope parity: both sides decode to Env with an
        // identical assistant_message.
        let with_ask =
            match decode_envelope(&with_digest, crate::core::workflow::TurnPurpose::Interview) {
                EnvelopeDecode::Env(env) => env.assistant_message.clone(),
                _ => None,
            };
        let minus_ask =
            match decode_envelope(&minus_digest, crate::core::workflow::TurnPurpose::Interview) {
                EnvelopeDecode::Env(env) => env.assistant_message.clone(),
                _ => None,
            };
        assert_eq!(
            with_ask,
            Some("Choices drafted; pick one before Friday.".to_string()),
            "{label}: digest-bearing reply did not decode to Env"
        );
        assert_eq!(
            minus_ask, with_ask,
            "{label}: EnvelopeDecode parity broken for the digest-minus pair"
        );
    }

    // Producer<->consumer link (story-002 detector): the prose the
    // paragraphs teach must classify under the shared detector with
    // the ask on the first bullet, and the closeout variant must read
    // as NoReply. The closeout phrase stays split in the source
    // spelling for the same exactly-once reason as in the capture test.
    let prose = "The SSO rollout draft is ready for a decision.";
    let example = format!(
        "{prose}\n---\n- Adopt one SSO route this week?\n- Recommend Option 1: least client work.\n- Comparison on the SSO task card."
    );
    let classified = parse_reply_tail(&example);
    assert_eq!(
        classified.kind,
        TailKind::Digest,
        "the taught digest example must classify as Digest"
    );
    assert_eq!(
        classified.bullets.len(),
        3,
        "the example must carry exactly its three prose bullets"
    );
    assert_eq!(
        classified.ask.as_deref(),
        Some("Adopt one SSO route this week?"),
        "ask must surface the first bullet VERBATIM"
    );
    assert_eq!(
        classified.ask.as_deref(),
        classified.bullets.first().map(String::as_str),
        "ask and the first bullet must coincide"
    );
    let closeout = format!("{prose} {}", concat!("No reply ", "needed."));
    let closeout_tail = parse_reply_tail(&closeout);
    assert!(
        closeout_tail.no_reply,
        "the closeout variant must read NoReply"
    );
    assert_eq!(closeout_tail.kind, TailKind::NoReply);
}
