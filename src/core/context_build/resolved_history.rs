use crate::{core::context_build::clip, domain::OpenItem};

const RECENT_ITEMS: usize = 8;
const SUMMARY_BUDGET: usize = 1_800;

pub(super) fn with_summary(open_items: String, resolved: &[OpenItem]) -> String {
    let summary = summarize(resolved);
    if summary.is_empty() {
        return open_items;
    }
    format!("{open_items}\n\nRecent completed item outcomes:\n{summary}")
}

fn summarize(resolved: &[OpenItem]) -> String {
    let entries = resolved
        .iter()
        .rev()
        .take(RECENT_ITEMS)
        .map(|item| {
            let mut entry = format!("- {}: {}", item.id, clip(&item.question, 48));
            if !item.recommendation.trim().is_empty() {
                entry.push_str(&format!("; answer: {}", clip(&item.recommendation, 48)));
            }
            if !item.evidence.trim().is_empty() {
                entry.push_str(&format!("; evidence: {}", clip(&item.evidence, 48)));
            }
            entry
        })
        .collect::<Vec<_>>()
        .join("\n");
    clip(&entries, SUMMARY_BUDGET)
}

#[cfg(test)]
mod tests {
    use crate::{domain::ItemKind, domain::OpenItem};

    use super::{SUMMARY_BUDGET, summarize, with_summary};

    #[test]
    fn resolved_history_is_recent_relevant_and_bounded() {
        let items = (0..12)
            .map(|n| {
                let mut item = OpenItem::new(
                    format!("CLR-{n:03}"),
                    crate::domain::Priority::Normal,
                    ItemKind::Question,
                    "General".into(),
                    None,
                    format!("Question {n} {}", "q".repeat(200)),
                    "why".into(),
                );
                item.recommendation = "chosen answer ".repeat(30);
                item.evidence = "supporting evidence ".repeat(30);
                item
            })
            .collect::<Vec<_>>();

        let summary = summarize(&items);
        assert!(summary.chars().count() <= SUMMARY_BUDGET);
        assert!(summary.contains("CLR-011"));
        assert!(summary.contains("CLR-004"));
        assert!(!summary.contains("CLR-003"));
        assert!(summary.contains("chosen answer"));
        assert!(summary.contains("supporting evidence"));
        assert!(!summary.contains("decisionBrief"));
    }

    #[test]
    fn no_resolved_items_add_no_history_section() {
        assert_eq!(with_summary("No open items".into(), &[]), "No open items");
    }
}
