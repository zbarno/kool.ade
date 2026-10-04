use crate::domain::{Authority, OpenItem};

pub(super) struct CardStatus<'a> {
    pub active: bool,
    pub expanded: bool,
    pub retry: bool,
    pub resolved: bool,
    pub ownership: bool,
    pub review: bool,
    pub blocked: bool,
    pub blocked_by: &'a [String],
    pub needs_answer: bool,
    pub reply_next: Option<&'a str>,
    pub item: Option<&'a OpenItem>,
    pub story: bool,
    pub implementation_column: usize,
    pub implementation_detail: Option<&'a str>,
    pub eligible: bool,
}

pub(super) fn labels(status: CardStatus<'_>) -> (&'static str, String) {
    let CardStatus {
        active,
        expanded,
        retry,
        resolved,
        ownership,
        review,
        blocked,
        blocked_by,
        needs_answer,
        reply_next,
        item,
        story,
        implementation_column,
        implementation_detail,
        eligible,
    } = status;
    let (heading, action) = if active {
        (
            if expanded {
                "Answer sent — Kool.ad/e is updating this task"
            } else {
                "Kool.ad/e is replying…"
            },
            "Your answer is saved. You can keep drafting while you wait.".to_string(),
        )
    } else if retry {
        (
            "Update not saved",
            "Nothing changed. Retry your last reply or send a revised answer.".to_string(),
        )
    } else if resolved {
        ("Resolved", "No reply needed.".to_string())
    } else if ownership {
        (
            "Your next step",
            "Choose who owns this category.".to_string(),
        )
    } else if review {
        (
            "Your next step",
            "Review Kool.ad/e’s recommendation and approve it or request a change.".to_string(),
        )
    } else if blocked {
        (
            "Waiting for a prerequisite",
            format!("Resolve {} first.", blocked_by.join(", ")),
        )
    } else if needs_answer {
        (
            "Your answer needed",
            reply_next
                .map(str::to_owned)
                .unwrap_or_else(|| item.unwrap().question.clone()),
        )
    } else if story {
        match implementation_column {
            4 => ("Completed", "No action needed.".to_string()),
            2 => (
                "Ready for review",
                "Review the changes in the pull request.".to_string(),
            ),
            3 => (
                "Needs attention",
                implementation_detail
                    .map(|detail| crate::core::context_build::clip(detail, 240))
                    .unwrap_or_default(),
            ),
            1 => (
                "Kool.ad/e is working",
                "Implementation is running. Activity is available below.".to_string(),
            ),
            _ => (
                "Ready to implement",
                "Start implementation, or add context before Kool.ad/e begins.".to_string(),
            ),
        }
    } else if !eligible {
        (
            "Waiting on the owner",
            format!(
                "Assigned to {}. You can still add context.",
                item.and_then(|i| i.assigned_to.as_deref())
                    .unwrap_or("another stakeholder")
            ),
        )
    } else if item.is_some_and(|i| i.authority == Authority::Agent) {
        (
            "Assigned to Kool.ad/e",
            "No answer needed from you.".to_string(),
        )
    } else {
        ("No reply needed", String::new())
    };
    (heading, action)
}
