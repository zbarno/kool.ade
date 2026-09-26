use super::*;

#[test]
fn malformed_specification_rejects_otherwise_valid_turn_without_mutation() {
    let (inputs, dir) = inputs_for("spec_layout", "rewrite the specification");
    git_stdout(&dir, &["add", ".kool-ade-packet"]);
    git_stdout(&dir, &["commit", "-m", "Seed planning artifacts"]);
    let before = law_evidence(&dir);
    let raw = serde_json::json!({
        "schema_version": 1,
        "assistant_message": "Revised the document.",
        "change_summary": "Revise specification",
        "updated_specification": "# Fixture\n\n## Audit notes\nIncomplete replacement.",
        "open_items_added": [{"kind":"Question", "priority":"Normal",
            "category":"General", "assigned_to":"All", "question":"Which platform?",
            "reason":"Defines launch scope"}]
    })
    .to_string();
    let controller = TurnController::start(
        inputs,
        Box::new(ScriptedHarness {
            canned: None,
            raw: Some(raw),
        }),
    );
    match drain(&controller) {
        TurnOutcome::Rejected { problems, .. } => {
            assert!(problems.iter().any(|p| p.contains("updated_specification")));
        }
        other => panic!("expected structural rejection, got {other:?}"),
    }
    assert_eq!(
        law_evidence(&dir),
        before,
        "no artifact or checkpoint may change"
    );
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn missing_block_rejects_without_side_effects() {
    let (inputs, dir) = inputs_for("noblock", "go");
    let c = TurnController::start(
        inputs,
        Box::new(ScriptedHarness {
            canned: None,
            raw: Some("chatty but no json 😅".into()),
        }),
    );
    match drain(&c) {
        TurnOutcome::Rejected { problems, .. } => {
            assert!(!problems.is_empty());
        }
        other => panic!("expected Rejected, got: {other:?}"),
    }
    let st = PlannerState::load(&dir).unwrap();
    assert!(st.items.is_empty());
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn bad_envelope_types_reject_cleanly() {
    let (inputs, dir) = inputs_for("badyes", "resolve CLR-999 please");
    // Envelope that claims to resolve an id that does not exist.
    let env = TurnEnvelope {
        schema_version: Some(2),
        assistant_message: Some("Okay!".into()),
        change_summary: None,
        document_updates: None,
        updated_specification: None,
        open_items_added: None,
        open_items_updated: None,
        open_items_resolved: Some(vec!["CLR-999".into()]),
        next_question_id: None,
        interview: None,
        task_stories: None,
        requested_action: None,
        task_outline: None,
    };
    let c = TurnController::start(
        inputs,
        Box::new(ScriptedHarness {
            canned: Some(env),
            raw: None,
        }),
    );
    match drain(&c) {
        TurnOutcome::Rejected { problems, .. } => {
            assert!(problems.iter().any(|p| p.contains("CLR-999")));
        }
        other => panic!("expected Rejected, got: {other:?}"),
    }
    let st = PlannerState::load(&dir).unwrap();
    assert!(st.items.is_empty());
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn cancel_api_toggles_predictably() {
    let (inputs, dir) = inputs_for("cancel", "think hard");
    let c = TurnController::start(
        inputs,
        Box::new(ScriptedHarness {
            canned: None,
            raw: Some("{}\n".into()),
        }),
    );
    assert!(!c.cancel_requested());
    c.request_cancel();
    assert!(c.cancel_requested());
    let _ = drain(&c);
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn live_previews_precede_completion_and_never_write_unvalidated_content() {
    for mode in ["success", "invalid", "cancel", "failure"] {
        let (inputs, dir) = inputs_for(&format!("stream_{mode}"), "draft scope");
        let before = inputs.state.spec_text.clone();
        let gate = Arc::new(std::sync::Barrier::new(2));
        let text = if mode == "invalid" {
            "no envelope".into()
        } else {
            // Snake-case is the actual prompt contract, camelCase remains supported.
            serde_json::json!({"schema_version":1, "assistant_message":"Draft saved.",
                    "document_updates":[{"document_id":"product:overview","content":"# Overview\n\nLive content\n"}], "open_items_added":[],
                    "open_items_updated":[], "open_items_resolved":[]})
                .to_string()
        };
        let c = TurnController::start(
            inputs,
            Box::new(StreamingHarness {
                gate: gate.clone(),
                text,
                fail: mode == "failure",
            }),
        );
        let mut previews = Vec::new();
        for _ in 0..2 {
            match c.poll(Duration::from_secs(3)) {
                Some(TurnEvt::Progress(p)) => previews.push(p.specification.unwrap()),
                _ => {
                    gate.wait();
                    panic!("expected live preview before completion");
                }
            }
        }
        assert_eq!(previews, ["# Draft", "# Draft\n\nLive content"]);
        assert_eq!(PlannerState::load(&dir).unwrap().spec_text, before);
        if mode == "cancel" {
            c.request_cancel();
        }
        gate.wait();
        let outcome = drain(&c);
        match mode {
            "success" => assert!(matches!(outcome, TurnOutcome::Applied { .. })),
            "invalid" => assert!(matches!(outcome, TurnOutcome::Rejected { .. })),
            _ => assert!(matches!(outcome, TurnOutcome::HarnessFailed { .. })),
        }
        if mode != "success" {
            assert_eq!(PlannerState::load(&dir).unwrap().spec_text, before);
        }
        assert!(
            c.poll(Duration::ZERO).is_none(),
            "no stale preview may arrive after completion"
        );
        let _ = std::fs::remove_dir_all(dir);
    }
}
