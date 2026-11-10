//! Plain-language status text for the canonical plan-confirmation card.
//!
//! Readers are nontechnical adults; the primary sentence always answers
//! "what do I need to decide or do?". Raw machine messages are translated,
//! never invented away.

/// Translate a raw readiness error into reader language, keeping the exact
/// machine reason available in parentheses for diagnostics.
pub(super) fn plain_blocker(raw: &str) -> String {
    let plain = if raw == "no persisted readiness assessment exists for the current plan" {
        "Kool.ad/e has not finished checking whether this plan is complete."
    } else if raw
        == "persisted readiness is missing or belongs to a different specification revision"
    {
        "The plan was edited after its last completeness check, so it is being re-checked."
    } else if raw == "the current specification is not marked ready" {
        "The plan still has unfinished requirements."
    } else if raw == "persisted readiness is stale relative to newer clarification records" {
        "Your recent answers post-date the last completeness check, so it is being re-checked."
    } else if raw == "persisted readiness evidence no longer passes the current specification" {
        "The completeness check no longer matches the current plan, so it is being re-checked."
    } else if raw.starts_with("persisted readiness could not be loaded") {
        "The plan's completeness record could not be read."
    } else if raw.starts_with("clarification records could not be loaded") {
        "The plan's recorded questions could not be read."
    } else {
        raw
    };
    if plain == raw {
        plain.to_owned()
    } else {
        format!("{plain} ({raw})")
    }
}

#[allow(clippy::too_many_arguments)]
pub(super) fn detail(
    ready: bool,
    feature_title: &str,
    blockers: &[String],
    hash: &str,
    spec_revision: &str,
    published: bool,
    confirmed: bool,
    drift: bool,
    review_failed: bool,
    approved: bool,
) -> String {
    let stamp = format!(
        "revision {}, snapshot {}",
        truncate(spec_revision, 24),
        &hash[..hash.len().min(12)]
    );
    let form = if published {
        "the published product document"
    } else {
        "an editable draft"
    };
    if !ready {
        let mut text = String::from("The plan is not ready for approval yet. ");
        if blockers.is_empty() {
            text.push_str("Working out why.");
        } else {
            text.push_str("Outstanding:\n");
            for blocker in blockers.iter().take(4) {
                text.push_str(&format!(" • {}\n", plain_blocker(blocker)));
            }
            if blockers.len() > 4 {
                text.push_str(&format!(" • …and {} more\n", blockers.len() - 4));
            }
        }
        text.push_str("This card moves to “In Review” by itself once the items above are settled.");
        if confirmed {
            text.push_str(
                " Your earlier approval pauses until the revised plan is approved again.",
            );
        }
        return text;
    }
    if confirmed && approved {
        return format!(
            "Approved: this exact plan ({stamp}) is the implementation baseline. \
             Implementation tasks are generated — continue with the task cards.",
        );
    }
    if confirmed && drift {
        return format!(
            "“{feature_title}” was revised after your earlier approval, so that \
             approval no longer applies. Review the current {form} ({stamp}) and \
             approve it again.",
        );
    }
    if confirmed && review_failed {
        return format!(
            "You approved this plan ({stamp}), and the independent review requested \
             changes. Decide the proposed repairs on the plan-review card, or press \
             “Request changes” here to revise the plan yourself.",
        );
    }
    if confirmed {
        return format!(
            "You approved this plan ({stamp}). Kool.ad/e is running the independent \
             review now; the next step appears when it finishes.",
        );
    }
    format!(
        "“{feature_title}” is {form} ({stamp}) and looks complete. Approving starts \
         Kool.ad/e's independent review and then task generation. Nothing proceeds \
         without your explicit approval — chatting about the plan never counts as \
         approving it.",
    )
}

fn truncate(value: &str, max: usize) -> &str {
    value.get(..max).unwrap_or(value)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn hash() -> String {
        "a".repeat(64)
    }

    fn args(
        ready: bool,
        confirmed: bool,
        drift: bool,
        review_failed: bool,
        approved: bool,
        blockers: Vec<&str>,
    ) -> String {
        detail(
            ready,
            "Login screen",
            &blockers.into_iter().map(str::to_owned).collect::<Vec<_>>(),
            &hash(),
            "abc123",
            false,
            confirmed,
            drift,
            review_failed,
            approved,
        )
    }

    #[test]
    fn ready_uncertain_answers_the_primary_question() {
        let detail = args(true, false, false, false, false, vec![]);
        assert!(detail.contains("looks complete"));
        assert!(detail.contains("never counts as approving it"));
        assert!(detail.contains("aaaaaaaaaa"));
    }

    #[test]
    fn blocked_lists_plain_language_blockers() {
        let detail = args(
            false,
            false,
            false,
            false,
            false,
            vec!["no persisted readiness assessment exists for the current plan"],
        );
        assert!(detail.contains("not ready for approval yet"));
        assert!(detail.contains("Kool.ad/e has not finished checking"));
    }

    #[test]
    fn drift_requires_fresh_approval() {
        let detail = args(true, true, true, false, false, vec![]);
        assert!(detail.contains("no longer applies"));
    }

    #[test]
    fn approved_terminal_summary() {
        let detail = args(true, true, false, false, true, vec![]);
        assert!(detail.contains("implementation baseline"));
    }

    #[test]
    fn review_failure_points_at_options() {
        let detail = args(true, true, false, true, false, vec![]);
        assert!(detail.contains("requested changes"));
        assert!(detail.contains("Request changes"));
    }
}
