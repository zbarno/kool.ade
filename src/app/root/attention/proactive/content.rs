use super::Kind;
use crate::core::attention::{self, Brief, HumanStep, OptionBrief};

pub(super) fn event_id(repo: &std::path::Path, ticket: &str, detail: &str, kind: &Kind) -> String {
    let mut material = detail.as_bytes().to_vec();
    if let Some(report) = attention::source_path(repo, ticket, detail)
        && let Ok(bytes) = std::fs::read(report)
    {
        material.extend(bytes);
    }
    material.extend_from_slice(ticket.as_bytes());
    material.push(match kind {
        Kind::Blocker => 0,
        Kind::Approval => 1,
        Kind::Clarification => 2,
    });
    format!("needs-attention:{}", fingerprint(&material))
}

pub(super) fn clarification_event_id(item: &crate::domain::OpenItem) -> String {
    let identity = item.uid.as_deref().unwrap_or(&item.id);
    format!(
        "needs-attention:clarification:{}",
        fingerprint(identity.as_bytes())
    )
}

pub(super) fn clarification_detail(item: &crate::domain::OpenItem) -> String {
    let decision = serde_json::to_string(&item.decision_brief).unwrap_or_default();
    format!("{}\n{}\n{decision}", item.question, item.reason)
}

pub(super) fn clarification_brief(item: &crate::domain::OpenItem) -> Option<Brief> {
    if item.authority != crate::domain::Authority::Human && !item.is_ownership_gap() {
        return None;
    }
    let decision = item.decision_brief.clone();
    let options = decision
        .as_ref()
        .map(|brief| {
            brief
                .options
                .iter()
                .map(|option| {
                    let consequence = [
                        option.consequences.join("; "),
                        option.costs.join("; "),
                        option.risks.join("; "),
                    ]
                    .into_iter()
                    .filter(|part| !part.is_empty())
                    .collect::<Vec<_>>()
                    .join("; ");
                    OptionBrief {
                        id: option.id.clone(),
                        label: option.label.clone(),
                        meaning: option.summary.clone(),
                        consequence: if consequence.is_empty() {
                            "Your answer will guide the related planning work.".into()
                        } else {
                            consequence
                        },
                        clarification: false,
                        source_evidence: None,
                    }
                })
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();
    let mut problem = format!("Kool.ad/e needs your decision: {}", item.question.trim());
    if !item.reason.trim().is_empty() {
        problem.push_str(&format!(" This matters because {}", item.reason.trim()));
    }
    Some(Brief {
        problem,
        recommendation: decision.and_then(|brief| {
            brief
                .recommendation
                .map(|recommendation| crate::core::attention::Recommendation {
                    option_id: recommendation.option_id,
                    rationale: recommendation.rationale,
                })
        }),
        options,
        steps: vec![HumanStep {
            owner: "You".into(),
            action:
                "Answer this question in the task conversation so Kool.ad/e can continue planning."
                    .into(),
        }],
        after: "Open this conversation, send your answer, and Kool.ad/e will update the plan."
            .into(),
    })
}

pub(super) fn approval_brief(ticket: &str) -> Brief {
    Brief {
        problem: "Kool.ad/e finished verifying this task and is waiting for your approval before it creates a pull request.".into(),
        recommendation: None,
        options: vec![OptionBrief {
            id: "approve".into(),
            label: "Approve".into(),
            meaning: "Allow Kool.ad/e to publish the verified changes as a pull request.".into(),
            consequence: "The changes become available for review on GitHub.".into(),
            clarification: false, source_evidence: Some(format!("Task {ticket} is awaiting approval.")),
        }],
        steps: vec![HumanStep {
            owner: "You".into(),
            action: "Review the task's changes, then approve them or request changes.".into(),
        }],
        after: "Close the task details, then choose Approve or Request changes on the task card."
            .into(),
    }
}

pub(super) fn fallback_brief(detail: &str) -> Brief {
    let problem = detail
        .split("### Next action(s)")
        .next()
        .unwrap_or(detail)
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty() && !line.starts_with('#'))
        .collect::<Vec<_>>()
        .join(" ");
    let actions = detail
        .split_once("### Next action(s)")
        .map(|(_, rest)| rest)
        .unwrap_or("");
    let steps = actions
        .lines()
        .map(str::trim)
        .filter_map(|line| line.strip_prefix("- "))
        .filter(|line| !line.starts_with("Full report:"))
        .map(|line| {
            let (owner, action) = line.split_once(':').unwrap_or(("You", line));
            HumanStep {
                owner: owner.trim().to_owned(),
                action: action.trim().to_owned(),
            }
        })
        .collect::<Vec<_>>();
    Brief {
        problem: if problem.is_empty() {
            "Kool.ad/e has paused this task and needs your help to continue.".into()
        } else {
            problem
        },
        recommendation: None,
        options: Vec::new(),
        steps: if steps.is_empty() {
            vec![HumanStep {
                owner: "You".into(),
                action: "Review the task details and choose the next action.".into(),
            }]
        } else {
            steps
        },
        after: "After completing the action, open the task details and choose Resume implementation to try again.".into(),
    }
}

fn fingerprint(bytes: &[u8]) -> String {
    let mut hash = 0xcbf29ce484222325u64;
    for byte in bytes {
        hash = (hash ^ u64::from(*byte)).wrapping_mul(0x100000001b3);
    }
    format!("{hash:016x}-{}", bytes.len())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_human_owned_items_can_be_rendered_as_user_decisions() {
        let mut agent = crate::domain::OpenItem::new(
            "CLR-001".into(),
            crate::domain::Priority::Blocking,
            crate::domain::ItemKind::Question,
            "General".into(),
            None,
            "Inspect the source.".into(),
            "Evidence is missing.".into(),
        );
        agent.authority = crate::domain::Authority::Agent;
        assert!(clarification_brief(&agent).is_none());

        agent.authority = crate::domain::Authority::Review;
        assert!(clarification_brief(&agent).is_none());

        agent.authority = crate::domain::Authority::Human;
        assert!(clarification_brief(&agent).is_some());
    }

    #[test]
    fn clarification_identity_does_not_depend_on_question_copy() {
        let mut item = crate::domain::OpenItem::new(
            "CLR-001".into(),
            crate::domain::Priority::High,
            crate::domain::ItemKind::Question,
            "General".into(),
            None,
            "Which branch should receive this task?".into(),
            "The task needs a destination.".into(),
        );
        let first = clarification_event_id(&item);
        item.question = "What is the destination branch?".into();
        item.reason = "This task needs its target.".into();
        assert_eq!(clarification_event_id(&item), first);
    }
}
