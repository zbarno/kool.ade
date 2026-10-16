use super::*;

fn delta(requirement_id: &str) -> SpecChangeV1 {
    SpecChangeV1 {
        requirement_id: requirement_id.into(),
        old: "The feature has no verified delivery status.".into(),
        new: "The feature's verified delivery status is recorded.".into(),
        classification: DeltaClassification::CurrentTruth,
        evidence: vec!["e1".into()],
        approval_effect: ApprovalEffect::DoesNotChangeApproval,
        followup_task_ids: vec!["task-17".into()],
    }
}

fn validation_context() -> OutputValidationContext<'static> {
    OutputValidationContext {
        required_requirements: vec!["REQ-1".into()],
        evidence_ids: std::collections::HashSet::from(["e1".into()]),
        ..Default::default()
    }
}

fn rejects_delta(value: SpecDeltaV1) {
    let json = serde_json::to_string(&value).unwrap();
    assert!(
        validate_output_with_context(PromptRole::SpecReconciler, &json, &validation_context(),)
            .is_err()
    );
}

#[test]
fn spec_reconciler_output_is_bound_to_requirements_and_manifest_evidence() {
    let valid = serde_json::to_string(&SpecDeltaV1 {
        deltas: vec![delta("REQ-1")],
    })
    .unwrap();
    let context = OutputValidationContext {
        required_requirements: vec!["REQ-1".into()],
        evidence_ids: std::collections::HashSet::from(["e1".into()]),
        ..Default::default()
    };
    assert!(matches!(
        validate_output_with_context(PromptRole::SpecReconciler, &valid, &context),
        Ok(RoleOutputV1::SpecDelta(_))
    ));

    let unknown_requirement = serde_json::to_string(&SpecDeltaV1 {
        deltas: vec![delta("REQ-2")],
    })
    .unwrap();
    assert!(
        validate_output_with_context(PromptRole::SpecReconciler, &unknown_requirement, &context,)
            .is_err()
    );

    let unknown_evidence = serde_json::to_string(&SpecDeltaV1 {
        deltas: vec![SpecChangeV1 {
            evidence: vec!["not-in-manifest".into()],
            ..delta("REQ-1")
        }],
    })
    .unwrap();
    assert!(
        validate_output_with_context(PromptRole::SpecReconciler, &unknown_evidence, &context,)
            .is_err()
    );
}

#[test]
fn spec_reconciler_rejects_duplicate_or_non_changes_and_accepts_no_delta() {
    let duplicate = serde_json::to_string(&SpecDeltaV1 {
        deltas: vec![delta("REQ-1"), delta("REQ-1")],
    })
    .unwrap();
    let context = OutputValidationContext {
        required_requirements: vec!["REQ-1".into()],
        evidence_ids: std::collections::HashSet::from(["e1".into()]),
        ..Default::default()
    };
    assert!(
        validate_output_with_context(PromptRole::SpecReconciler, &duplicate, &context).is_err()
    );

    let unchanged = serde_json::to_string(&SpecDeltaV1 {
        deltas: vec![SpecChangeV1 {
            new: "The feature has no verified delivery status.".into(),
            ..delta("REQ-1")
        }],
    })
    .unwrap();
    assert!(
        validate_output_with_context(PromptRole::SpecReconciler, &unchanged, &context).is_err()
    );

    let no_changes = serde_json::to_string(&SpecDeltaV1 { deltas: vec![] }).unwrap();
    assert!(
        validate_output_with_context(PromptRole::SpecReconciler, &no_changes, &context).is_ok()
    );

    let missing_requirement_context = OutputValidationContext {
        evidence_ids: std::collections::HashSet::from(["e1".into()]),
        ..Default::default()
    };
    let unbound_delta = serde_json::to_string(&SpecDeltaV1 {
        deltas: vec![delta("REQ-1")],
    })
    .unwrap();
    assert!(
        validate_output_with_context(
            PromptRole::SpecReconciler,
            &unbound_delta,
            &missing_requirement_context,
        )
        .is_err()
    );
    assert!(
        validate_output_with_context(
            PromptRole::SpecReconciler,
            &no_changes,
            &missing_requirement_context,
        )
        .is_ok()
    );
}

#[test]
fn spec_reconciler_enforces_delta_and_field_bounds() {
    rejects_delta(SpecDeltaV1 {
        deltas: (0..=128).map(|_| delta("REQ-1")).collect(),
    });

    for requirement_id in [String::new(), "   ".into(), "R".repeat(129)] {
        rejects_delta(SpecDeltaV1 {
            deltas: vec![delta(&requirement_id)],
        });
    }

    for old in [String::new(), "  \t".into(), "o".repeat(8193)] {
        rejects_delta(SpecDeltaV1 {
            deltas: vec![SpecChangeV1 {
                old,
                ..delta("REQ-1")
            }],
        });
    }
    for new in [String::new(), "  \n".into(), "n".repeat(8193)] {
        rejects_delta(SpecDeltaV1 {
            deltas: vec![SpecChangeV1 {
                new,
                ..delta("REQ-1")
            }],
        });
    }

    rejects_delta(SpecDeltaV1 {
        deltas: vec![SpecChangeV1 {
            evidence: vec![],
            ..delta("REQ-1")
        }],
    });
    rejects_delta(SpecDeltaV1 {
        deltas: vec![SpecChangeV1 {
            followup_task_ids: (0..=64).map(|n| format!("task-{n}")).collect(),
            ..delta("REQ-1")
        }],
    });
    for followup_task_id in [String::new(), " \t".into(), "t".repeat(129)] {
        rejects_delta(SpecDeltaV1 {
            deltas: vec![SpecChangeV1 {
                followup_task_ids: vec![followup_task_id],
                ..delta("REQ-1")
            }],
        });
    }
}

#[test]
fn spec_reconciler_requirement_matching_is_exact_and_case_sensitive() {
    // Requirement identifiers are opaque strings: there is no case folding or
    // canonicalization in the catalog validator, so only the approved spelling matches.
    rejects_delta(SpecDeltaV1 {
        deltas: vec![delta("req-1")],
    });
    let case_distinct = serde_json::to_string(&SpecDeltaV1 {
        deltas: vec![delta("REQ-1"), delta("req-1")],
    })
    .unwrap();
    let context = OutputValidationContext {
        required_requirements: vec!["REQ-1".into(), "req-1".into()],
        evidence_ids: std::collections::HashSet::from(["e1".into()]),
        ..Default::default()
    };
    assert!(
        validate_output_with_context(PromptRole::SpecReconciler, &case_distinct, &context).is_ok()
    );
}
