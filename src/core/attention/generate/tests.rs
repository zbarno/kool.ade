use super::*;
use crate::core::implementation::{BlockerDisposition, ReportStatus};

#[test]
fn prompt_requires_issue_specific_effects_and_all_choices() {
    let report = Report {
        status: ReportStatus::Blocked,
        blocker_disposition: BlockerDisposition::HumanAction,
        summary: "Published history conflicts with the file list".into(),
        acceptance_criteria: vec![crate::core::implementation::report::Criterion {
            criterion: "The frozen-base table has eight paths".into(),
            evidence: "The published base contains 26 paths, including 19 added later.".into(),
        }],
        verification: vec!["Replay reproduced the 26-row footprint.".into()],
        remaining: vec!["Adjudicator: choose (a) accept or (b) revise".into()],
        human_choices: Vec::new(),
    };
    let text = prompt(&report, "task story", "ledger", "");
    assert!(text.contains("preserve every choice ID"));
    assert!(text.contains("source_evidence"));
    assert!(text.contains("Do not treat sequential steps as choices"));
    assert!(text.contains("recommendation=null"));
    assert!(text.contains("TASK STORY (scope and user intent):"));
    assert!(text.contains("ledger"));
    assert!(text.contains("The frozen-base table has eight paths"));
    assert!(text.contains("published base contains 26 paths"));
    assert!(text.contains("Replay reproduced the 26-row footprint"));
}

#[test]
fn cache_material_includes_the_current_instructions_and_task_evidence() {
    let report = Report {
        status: ReportStatus::Blocked,
        blocker_disposition: BlockerDisposition::HumanAction,
        summary: "Published history conflicts with the file list".into(),
        acceptance_criteria: vec![],
        verification: vec![],
        remaining: vec!["Adjudicator: choose (a) accept or (b) revise".into()],
        human_choices: Vec::new(),
    };
    let material = cache_material(
        &report,
        "The specific user task",
        "Specific repository evidence",
    );
    assert!(material.contains("Explain this saved implementation blocker"));
    assert!(material.contains("The specific user task"));
    assert!(material.contains("Specific repository evidence"));
    assert!(material.contains("Published history conflicts with the file list"));
    assert!(material.contains(
        "infer buttons only when one saved human-action line clearly offers multiple alternatives"
    ));
}

#[test]
fn prompt_carries_current_issue_alternatives_and_separate_operator_blocker() {
    let report = Report {
        status: ReportStatus::Blocked,
        blocker_disposition: BlockerDisposition::HumanAction,
        summary: "The account has reached its daily request quota; the provider resets it tomorrow.".into(),
        acceptance_criteria: vec![crate::core::implementation::report::Criterion {
            criterion: "The background report job completes before the daily reset.".into(),
            evidence: "The quota is exhausted, and the account owner must pick one recovery path.".into(),
        }],
        verification: vec!["All executable checks passed; the provider rejected the next request for quota.".into()],
        remaining: vec![
            "Account owner: choose one: wait for the monthly reset; request a temporary quota increase; reduce the batch size.".into(),
            "Operator: check the provider dashboard after the request; this requires an account login.".into(),
        ],
        human_choices: Vec::new(),
    };
    let text = prompt(&report, "ticket story", "reconcile-record.md", "");
    for evidence in [
        "daily request quota",
        "wait for the monthly reset",
        "request a temporary quota increase",
        "reduce the batch size",
        "provider dashboard",
    ] {
        assert!(
            text.contains(evidence),
            "prompt omitted issue evidence: {evidence}"
        );
    }
}
