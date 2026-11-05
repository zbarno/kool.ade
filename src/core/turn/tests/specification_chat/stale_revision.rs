use super::*;

/// Production TurnExecutor boundary: the published Git tree advances after
/// context capture but before generation returns. A validator unit test cannot
/// prove the post-generation guard runs before draft persistence. Uses only a
/// temporary Git fixture and local harness (no external services; low cost).
struct AdvancePublishedSpec {
    root: std::path::PathBuf,
    response: String,
}

impl AiHarness for AdvancePublishedSpec {
    fn label(&self) -> String {
        "advance-published-spec-test".into()
    }

    fn check_available(&self) -> Result<String, AppError> {
        Ok("fixture".into())
    }

    fn execute(&self, _request: &PlanningRequest) -> Result<HarnessOutcome, AppError> {
        let path = self.root.join("docs/koolade/product/overview.md");
        let mut content = std::fs::read_to_string(&path).unwrap();
        content.push_str("\n<!-- newer published revision -->\n");
        std::fs::write(path, content).unwrap();
        for args in [
            ["add", "docs/koolade/product/overview.md"].as_slice(),
            [
                "-c",
                "user.name=Kool.ad/e Test",
                "-c",
                "user.email=koolade-test@example.invalid",
                "commit",
                "-m",
                "advance fixture spec",
            ]
            .as_slice(),
        ] {
            let output = std::process::Command::new("git")
                .args(args)
                .current_dir(&self.root)
                .output()
                .unwrap();
            assert!(
                output.status.success(),
                "{}",
                String::from_utf8_lossy(&output.stderr)
            );
        }
        Ok(HarnessOutcome {
            final_text: self.response.clone(),
            envelope: None,
            stderr_tail: String::new(),
        })
    }
}

fn proposal_envelope(include_publish_request: bool) -> String {
    let mut envelope = serde_json::json!({
        "schema_version": 1,
        "assistant_message": "Prepared a local proposal.",
        "document_updates": [{
            "document_id": "product:overview",
            "content": "# Overview\n\nChanged proposal.\n"
        }],
        "open_items_added": [],
        "open_items_updated": [],
        "open_items_resolved": []
    });
    if include_publish_request {
        envelope["requested_action"] = serde_json::json!({"action": "publish"});
    }
    envelope.to_string()
}

fn specification_request(
    inputs: &TurnInputs,
    intent: crate::core::specification_chat::Intent,
) -> crate::core::specification_chat::Request {
    crate::core::specification_chat::Request {
        text: inputs.user_message.clone(),
        intent,
        target: target(inputs),
        omitted_history_messages: 0,
    }
}

fn assert_rejected(outcome: TurnOutcome, expected: &str) {
    let TurnOutcome::Rejected { problems, .. } = outcome else {
        panic!("expected the Specification turn to be rejected; got {outcome:?}")
    };
    assert!(
        problems.iter().any(|problem| problem.contains(expected)),
        "unexpected rejection: {problems:?}"
    );
}

#[test]
fn published_revision_drift_during_question_is_rejected() {
    let (mut inputs, root) = inputs_for("spec_chat_stale_ask", "Explain this product.");
    inputs.purpose = crate::core::workflow::TurnPurpose::Question;
    inputs.specification_chat = Some(specification_request(
        &inputs,
        crate::core::specification_chat::Intent::Ask,
    ));
    let result = execute::run_turn_read_only(
        &inputs,
        &AdvancePublishedSpec {
            root: root.clone(),
            response: "A stale answer should be rejected.".into(),
        },
        &Arc::new(std::sync::atomic::AtomicBool::new(false)),
        channel().0,
        None,
        None,
        Duration::from_secs(30),
    );
    assert_rejected(result, "published specifications changed");
    assert!(
        crate::core::apply::load_current(&inputs.state.planning_store)
            .unwrap()
            .is_none()
    );
    let _ = std::fs::remove_dir_all(&root);
    let _ = std::fs::remove_dir_all(root.with_extension("private-state"));
}

#[test]
fn published_revision_drift_during_proposal_creates_no_draft_or_source_commit() {
    let (mut inputs, root) = inputs_for("spec_chat_stale_proposal", "Propose a clearer overview.");
    let before_head = git_stdout(&root, &["rev-parse", "HEAD"]);
    inputs.specification_chat = Some(specification_request(
        &inputs,
        crate::core::specification_chat::Intent::Propose,
    ));
    let result = execute::run_turn(
        &inputs,
        &AdvancePublishedSpec {
            root: root.clone(),
            response: proposal_envelope(false),
        },
        &Arc::new(std::sync::atomic::AtomicBool::new(false)),
        channel().0,
        None,
        None,
        Duration::from_secs(30),
    );
    assert_rejected(result, "published specifications changed");
    assert!(
        crate::core::apply::load_current(&inputs.state.planning_store)
            .unwrap()
            .is_none()
    );
    assert_ne!(git_stdout(&root, &["rev-parse", "HEAD"]), before_head);
    assert_eq!(
        git_stdout(&root, &["log", "-1", "--format=%s"]).trim(),
        "advance fixture spec",
        "the harness revision is the only source commit"
    );
    let _ = std::fs::remove_dir_all(&root);
    let _ = std::fs::remove_dir_all(root.with_extension("private-state"));
}

#[test]
fn proposal_response_cannot_turn_a_local_draft_into_a_publish_action() {
    let (mut inputs, root) =
        inputs_for("spec_chat_publish_injection", "Propose a clearer overview.");
    let before_head = git_stdout(&root, &["rev-parse", "HEAD"]);
    inputs.specification_chat = Some(specification_request(
        &inputs,
        crate::core::specification_chat::Intent::Propose,
    ));
    let result = execute::run_turn(
        &inputs,
        &CaptureHarness {
            raw: proposal_envelope(true),
            sink: Arc::new(std::sync::Mutex::new(None)),
        },
        &Arc::new(std::sync::atomic::AtomicBool::new(false)),
        channel().0,
        None,
        None,
        Duration::from_secs(30),
    );
    assert_rejected(result, "cannot be combined with planning changes");
    assert!(
        crate::core::apply::load_current(&inputs.state.planning_store)
            .unwrap()
            .is_none()
    );
    assert_eq!(git_stdout(&root, &["rev-parse", "HEAD"]), before_head);
    let _ = std::fs::remove_dir_all(&root);
    let _ = std::fs::remove_dir_all(root.with_extension("private-state"));
}

#[test]
fn malformed_proposal_output_creates_no_draft_or_source_commit() {
    let (mut inputs, root) = inputs_for(
        "spec_chat_malformed_proposal",
        "Propose a clearer overview.",
    );
    let before_head = git_stdout(&root, &["rev-parse", "HEAD"]);
    inputs.specification_chat = Some(specification_request(
        &inputs,
        crate::core::specification_chat::Intent::Propose,
    ));
    let result = execute::run_turn(
        &inputs,
        &CaptureHarness {
            raw: "this is not a valid Specification response".into(),
            sink: Arc::new(std::sync::Mutex::new(None)),
        },
        &Arc::new(std::sync::atomic::AtomicBool::new(false)),
        channel().0,
        None,
        None,
        Duration::from_secs(30),
    );
    assert!(
        matches!(result, TurnOutcome::Rejected { .. }),
        "malformed proposal output must fail closed: {result:?}"
    );
    assert!(
        crate::core::apply::load_current(&inputs.state.planning_store)
            .unwrap()
            .is_none()
    );
    assert_eq!(git_stdout(&root, &["rev-parse", "HEAD"]), before_head);
    let _ = std::fs::remove_dir_all(&root);
    let _ = std::fs::remove_dir_all(root.with_extension("private-state"));
}
