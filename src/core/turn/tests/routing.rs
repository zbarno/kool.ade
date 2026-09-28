use super::*;

#[test]
fn routing_law_rejects_misroute_with_zero_mutation_and_applies_seat_inheritance() {
    let (mut inputs, dir) = inputs_for("routing-law", "Clarify the security and infosec lanes");

    // Shape the configuration: Security is SOLE-owned by Priya; InfoSec
    // is unowned — an entry EXISTS with an EMPTY member list, the exact
    // seeded-repo shape (must behave identically to a missing entry).
    {
        let mut st = inputs.state.clone();
        st.config.user = None;
        st.config.stakeholders = Stakeholders::new(vec![
            CategoryOwners::new("Security", vec!["Priya".into()]),
            CategoryOwners::new("InfoSec", Vec::new()),
        ]);
        std::fs::write(
            dir.join(crate::artifacts::CONFIG_FILE),
            config_io::serialize(&st.config),
        )
        .unwrap();
        st.resync().unwrap();
        // Ticket 1's seat: the connected repo's git user (non-guest),
        // so seat inheritance can fire for this operator. "Packet Test"
        // stands in for the ticket's seated operator Zach: derived from
        // the connected repository's git config (not the config block or
        // (guest)), and a member of none of the lane-holder lists.
        assert_eq!(st.effective_user().name, "Packet Test");
        inputs.state = st;
    }

    // Seed one open Question item per lane through a scripted turn.
    let seed = TurnEnvelope {
        schema_version: Some(2),
        assistant_message: Some("Opened the lane-bound questions.".into()),
        change_summary: Some("raise security and infosec lane questions".into()),
        document_updates: Some(vision_update("Seeded for the routing-law demonstration.")),
        updated_specification: None,
        open_items_added: Some(vec![
            TurnItem {
                authority: None,
                id: None,
                kind: Some("Question".into()),
                category: Some("Security".into()),
                assigned_to: Some("Priya".into()),
                priority: Some("Blocking".into()),
                question: Some("How deep must the threat model go?".into()),
                reason: Some("audit depth drives scope".into()),
                resolution_note: None,
                feature_id: None,
                recommendation: None,
                evidence: None,
                decision_brief: None,
                blocked_by: Vec::new(),
            },
            TurnItem {
                authority: None,
                id: None,
                kind: Some("Question".into()),
                category: Some("InfoSec".into()),
                assigned_to: Some("All".into()),
                priority: Some("Blocking".into()),
                question: Some("What is the incident-response cadence?".into()),
                reason: Some("operational exposure".into()),
                resolution_note: None,
                feature_id: None,
                recommendation: None,
                evidence: None,
                decision_brief: None,
                blocked_by: Vec::new(),
            },
        ]),
        open_items_updated: None,
        open_items_resolved: None,
        next_question_id: None,
        interview: None,
        task_stories: None,
        requested_action: None,
        follow_up_task: None,
        task_outline: None,
        plans: None,
        recommendation: None,
    };
    let ctrl = TurnController::start(
        inputs.clone(),
        Box::new(ScriptedHarness {
            canned: Some(seed),
            raw: None,
        }),
    );
    // Chain the applied state forward exactly like the app's tick loop:
    // each subsequent turn must start from the PREVIOUS outcome's state,
    // never from inputs' original snapshot.
    match drain(&ctrl) {
        TurnOutcome::Applied { state, .. } => inputs.state = *state,
        other => panic!("seed turn should apply, got: {other:?}"),
    }
    // Sanity straight off disk: both lanes seeded with distinct ids.
    let seeded = PlannerState::load(&dir).unwrap();
    assert_eq!(seeded.items.len(), inputs.state.items.len());
    let sec_id = seeded
        .items
        .iter()
        .find(|i| i.category.eq_ignore_ascii_case("Security"))
        .expect("security item seeded")
        .id
        .clone();
    let info_id = seeded
        .items
        .iter()
        .find(|i| i.category.eq_ignore_ascii_case("InfoSec") && i.kind != ItemKind::Ownership)
        .expect("infosec question seeded")
        .id
        .clone();
    let before = law_evidence(&dir);
    assert!(before.3 >= 1, "the seed turn must have checkpointed");

    // MISROUTE: a valid spec replacement plus next_question_id pointing
    // at the SOLE-OWNED Security item → the whole turn is rejected.
    let misroute = TurnEnvelope {
        schema_version: Some(2),
        assistant_message: Some("Recorded the security decision; carrying on.".into()),
        change_summary: Some("record threat-model depth decision".into()),
        document_updates: Some(vision_update("Threat model settled to L2.")),
        updated_specification: None,
        open_items_added: None,
        open_items_updated: None,
        open_items_resolved: None,
        next_question_id: Some(sec_id.clone()),
        interview: None,
        task_stories: None,
        requested_action: None,
        follow_up_task: None,
        task_outline: None,
        plans: None,
        recommendation: None,
    };
    let ctrl = TurnController::start(
        inputs.clone(),
        Box::new(ScriptedHarness {
            canned: Some(misroute),
            raw: None,
        }),
    );
    match drain(&ctrl) {
        TurnOutcome::Rejected { problems, .. } => {
            assert!(
                problems
                    .iter()
                    .any(|p| p.contains(&sec_id) && p.contains("violates the routing law")),
                "expected the routing-law fatal naming {sec_id}, got: {problems:?}"
            );
        }
        other => panic!("misrouted envelope must be Rejected, got: {other:?}"),
    }
    // Zero mutation: every planning artifact byte-identical, no new commit.
    assert_eq!(
        law_evidence(&dir),
        before,
        "the rejected turn must leave canonical product, open-items, config, and the commit chain byte-identical"
    );

    // PAIR: the equivalently shaped envelope proposes the UNOWNED
    // InfoSec item — seat inheritance makes it lawful for the seated
    // git-identified operator, and the turn applies with EXACTLY ONE
    // new imperative-subject checkpoint whose subject derives from
    // this change summary.
    const CADENCE_SUMMARY: &str = "record incident-response cadence";
    let inherited = TurnEnvelope {
        schema_version: Some(2),
        assistant_message: Some("Recorded the infosec decision; carrying on.".into()),
        change_summary: Some(CADENCE_SUMMARY.into()),
        document_updates: Some(vision_update("Incident cadence: page within the hour.")),
        updated_specification: None,
        open_items_added: None,
        open_items_updated: None,
        open_items_resolved: None,
        next_question_id: Some(info_id.clone()),
        interview: None,
        task_stories: None,
        requested_action: None,
        follow_up_task: None,
        task_outline: None,
        plans: None,
        recommendation: None,
    };
    let ctrl = TurnController::start(
        inputs.clone(),
        Box::new(ScriptedHarness {
            canned: Some(inherited),
            raw: None,
        }),
    );
    match drain(&ctrl) {
        TurnOutcome::Applied {
            normalized,
            commit_result,
            state,
            ..
        } => {
            assert_eq!(
                normalized.next_question_id.as_deref(),
                Some(info_id.as_str()),
                "the seat-inherited next question must be preserved"
            );
            assert!(commit_result.is_ok(), "commit failed: {commit_result:?}");
            // In-memory adoption: the returned state carries the turn's
            // specification change verbatim, and the posed question
            // remains in its queue.
            assert!(
                state
                    .spec_text
                    .as_deref()
                    .is_some_and(|spec| spec.contains("Incident cadence: page within the hour.")),
                "applied in-memory state must adopt the module change"
            );
            assert!(
                state
                    .items
                    .iter()
                    .any(|i| i.id == info_id && i.kind == ItemKind::Question),
                "in-memory state must still carry the posed question"
            );
            inputs.state = *state;
        }
        other => panic!("seat-inherited envelope must be Applied, got: {other:?}"),
    }
    let after = law_evidence(&dir);
    assert_eq!(
        after.3,
        before.3 + 1,
        "exactly one new checkpoint for the accepted turn"
    );
    assert_eq!(
        git_stdout(&dir, &["log", "-1", "--pretty=%s"]),
        format!("planner: {CADENCE_SUMMARY}\n"),
        "checkpoint subject must derive from the turn's imperative change summary"
    );
    let _ = std::fs::remove_dir_all(&dir);
}
