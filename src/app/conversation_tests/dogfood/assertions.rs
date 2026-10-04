use super::Scenario;
use std::path::Path;

pub(super) fn check_outcome(
    scenario: Scenario,
    root: &Path,
    work: &crate::core::planning_work::Work,
    evidence: &serde_json::Value,
) {
    match scenario.code {
        "A" | "B" | "F" | "G" | "H" => check_feature_outcome(scenario, root, work, evidence),
        "C" => {
            let state = crate::core::state::PlannerState::load(root).unwrap();
            let feature = state
                .active_features
                .iter()
                .find(|(id, _)| Some(id.as_str()) == work.feature_id.as_deref())
                .expect("bug planning should persist its change specification");
            let metadata = crate::domain::ChangeMetadata::require_markdown(&feature.1).unwrap();
            assert_eq!(metadata.status, crate::domain::ChangeStatus::Ready);
        }
        "D" => {
            assert_eq!(work.status, crate::core::planning_work::WorkStatus::Done);
            assert!(
                crate::core::state::PlannerState::load(root)
                    .unwrap()
                    .active_features
                    .is_empty()
            );
        }
        "E" => assert!(
            work.follow_up_task.is_some(),
            "Question should offer user-selected related Feature work"
        ),
        _ => unreachable!(),
    }
}

fn check_feature_outcome(
    scenario: Scenario,
    root: &Path,
    work: &crate::core::planning_work::Work,
    evidence: &serde_json::Value,
) {
    assert!(
        work.feature_id.is_some(),
        "feature scenario did not create a typed feature; inspect the recorded conversation"
    );
    let state = crate::core::state::PlannerState::load(root).unwrap();
    assert!(
        !state.active_features.is_empty(),
        "feature specification should be saved"
    );
    match scenario.code {
        "A" => {
            let feature = state
                .active_features
                .iter()
                .find(|(id, _)| Some(id.as_str()) == work.feature_id.as_deref())
                .expect("Scenario A must save its feature specification");
            assert!(
                feature.1.contains("src/") || feature.1.contains(".koolade-packet/"),
                "Scenario A feature must include evidence grounded in the repository"
            );
        }
        "B" => check_ambiguous_decisions(&state, work, evidence),
        "F" => check_independent_decisions(&state, work),
        "G" => check_dependent_decisions(&state, work),
        "H" => {
            assert!(
                evidence["board_attention_ids"]
                    .as_array()
                    .unwrap()
                    .is_empty(),
                "repository-resolvable implementation details should not create attention cards"
            );
            assert!(
                !work.detail.contains('?'),
                "delegated shortcut choice should not ask for confirmation"
            );
            let feature = state
                .active_features
                .iter()
                .find(|(id, _)| Some(id.as_str()) == work.feature_id.as_deref())
                .expect("shortcut feature spec should be saved");
            let metadata = crate::domain::ChangeMetadata::require_markdown(&feature.1).unwrap();
            assert_eq!(metadata.status, crate::domain::ChangeStatus::Ready);
        }
        _ => {}
    }
}

fn check_ambiguous_decisions(
    state: &crate::core::state::PlannerState,
    work: &crate::core::planning_work::Work,
    evidence: &serde_json::Value,
) {
    let items: Vec<_> = state
        .items
        .iter()
        .filter(|item| {
            item.feature_id.as_deref() == work.feature_id.as_deref()
                && matches!(
                    item.authority,
                    crate::domain::Authority::Human | crate::domain::Authority::Review
                )
        })
        .collect();
    assert!(
        items.len() >= 2,
        "ambiguous accounts work should expose multiple user decisions"
    );
    let board_ids: Vec<&str> = evidence["board_attention_ids"]
        .as_array()
        .expect("board attention IDs should be captured")
        .iter()
        .filter_map(serde_json::Value::as_str)
        .collect();
    for item in items {
        assert!(
            item.blocked_by.is_empty(),
            "currently actionable ambiguity `{}` must be parallel",
            item.id
        );
        assert!(
            board_ids.contains(&item.id.as_str()),
            "actionable decision `{}` must appear in Needs Attention",
            item.id
        );
        let brief = item
            .decision_brief
            .as_ref()
            .unwrap_or_else(|| panic!("decision `{}` must explain its choices", item.id));
        let recommendation = brief
            .recommendation
            .as_ref()
            .unwrap_or_else(|| panic!("decision `{}` needs an advisory recommendation", item.id));
        assert!(
            brief
                .options
                .iter()
                .any(|option| option.id == recommendation.option_id),
            "decision `{}` must recommend one of its displayed options",
            item.id
        );
    }
}

fn check_independent_decisions(
    state: &crate::core::state::PlannerState,
    work: &crate::core::planning_work::Work,
) {
    for decision in ["storage", "notification", "theme"] {
        let matches: Vec<_> = state
            .items
            .iter()
            .filter(|item| {
                let question = item.question.to_lowercase();
                let matches_dimension = match decision {
                    "storage" => ["storage", "persist", "persistence", "store"]
                        .iter()
                        .any(|term| question.contains(term)),
                    "notification" => ["channel", "delivery", "trigger", "frequency", "cadence"]
                        .iter()
                        .any(|term| question.contains(term)),
                    "theme" => {
                        ["theme", "appearance", "visual style"]
                            .iter()
                            .any(|term| question.contains(term))
                            && !["storage", "persist", "store"]
                                .iter()
                                .any(|term| question.contains(term))
                    }
                    _ => unreachable!(),
                };
                item.feature_id.as_deref() == work.feature_id.as_deref()
                    && matches_dimension
                    && matches!(
                        item.authority,
                        crate::domain::Authority::Human | crate::domain::Authority::Review
                    )
            })
            .collect();
        assert_eq!(
            matches.len(),
            1,
            "independent `{decision}` decision must appear once as a user-actionable item"
        );
        assert!(
            matches[0].blocked_by.is_empty(),
            "independent `{decision}` decision must be immediately actionable"
        );
    }
    let detail = work.detail.to_lowercase();
    assert!(
        detail.contains("any order"),
        "assistant message must tell the user the independent decisions can be answered in any order"
    );
    assert!(
        !detail.contains("once that lands")
            && !detail.contains("split the other")
            && !detail.contains("wait until you answer"),
        "assistant message must not make independent choices sound sequential"
    );
}

fn check_dependent_decisions(
    state: &crate::core::state::PlannerState,
    work: &crate::core::planning_work::Work,
) {
    assert!(
        state.items.iter().any(|item| {
            item.feature_id.as_deref() == work.feature_id.as_deref()
                && item.question.to_lowercase().contains("storage")
        }),
        "storage prerequisite should be surfaced"
    );
    assert!(
        state.items.iter().all(|item| {
            item.feature_id.as_deref() != work.feature_id.as_deref()
                || !item.question.to_lowercase().contains("auth")
        }),
        "authentication detail must remain hidden until storage is resolved"
    );
}
