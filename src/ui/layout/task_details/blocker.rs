//! A short human-facing view of an implementation report's external actions.
use super::reply;

#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) struct Choice {
    pub code: char,
    pub label: String,
    pub detail: String,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) struct Plan {
    pub summary: String,
    pub steps: Vec<String>,
    pub choices: Vec<Choice>,
}

pub(super) fn parse(detail: &str) -> Option<Plan> {
    if !detail.starts_with("## Waiting for user action") {
        return None;
    }
    let actions = reply::failure_actions(detail);
    let decision = actions
        .iter()
        .find(|action| action.starts_with("Adjudicator:") || action.starts_with("Contract owner:"));
    let operator = actions
        .iter()
        .find(|action| action.starts_with("Operator:"));
    let choices = decision.map(|action| choices(action)).unwrap_or_default();
    let mut steps = Vec::new();
    if let Some(action) = decision {
        if choices.is_empty() {
            steps.push(short_action(action));
        } else {
            steps.push("Adjudicator: choose one remedy below and send your decision.".into());
        }
    }
    if let Some(action) = operator {
        if action.contains("L1") && action.contains("L3") && action.contains("L4") {
            steps.push("Operator: run GUI checks L1, L3 and L4 on a Linux desktop; save the transcript outside the repository.".into());
        } else {
            steps.push(short_action(action));
        }
    }
    for action in &actions {
        if !action.starts_with("Adjudicator:")
            && !action.starts_with("Contract owner:")
            && !action.starts_with("Operator:")
            && !action.starts_with("Packet")
        {
            steps.push(short_action(action));
        }
    }
    let summary = match (decision.is_some(), operator.is_some()) {
        (true, true) => "Implementation is paused for a decision and desktop checks.",
        (true, false) => "Implementation is paused for a decision.",
        (false, true) => "Implementation is paused for operator checks.",
        (false, false) => "Implementation is paused for an external action.",
    };
    Some(Plan {
        summary: summary.into(),
        steps,
        choices,
    })
}

fn short_action(action: &str) -> String {
    let first = action.split("; expected result:").next().unwrap_or(action);
    let first = first.split("; then ").next().unwrap_or(first);
    let words = first
        .split_whitespace()
        .take(24)
        .collect::<Vec<_>>()
        .join(" ");
    if words.len() < first.len() {
        format!("{words}…")
    } else {
        words
    }
}

fn choices(action: &str) -> Vec<Choice> {
    let markers = ('a'..='f')
        .filter_map(|code| action.find(&format!("({code})")).map(|at| (at, code)))
        .collect::<Vec<_>>();
    if !(2..=6).contains(&markers.len()) {
        return Vec::new();
    }
    let mut found = Vec::new();
    for (index, &(at, code)) in markers.iter().enumerate() {
        let end = markers
            .get(index + 1)
            .map(|(next, _)| *next)
            .unwrap_or(action.len());
        let detail = action[at + 3..end]
            .split(" - and fill ")
            .next()
            .unwrap_or_default()
            .split("; expected result:")
            .next()
            .unwrap_or_default()
            .trim()
            .trim_end_matches([',', '.', ' ', '-']);
        let detail = detail.strip_suffix(" or").unwrap_or(detail).to_owned();
        if detail.is_empty() {
            return Vec::new();
        }
        found.push(Choice {
            code,
            label: label(code, &detail),
            detail,
        });
    }
    found
}

fn label(code: char, detail: &str) -> String {
    let lower = detail.to_ascii_lowercase();
    let short = if lower.contains("ratify the effective base") {
        "Ratify effective base"
    } else if lower.contains("reissue the corrected footprint predicate") {
        "Correct the footprint rule"
    } else if lower.contains("sanction explicit exemptions") {
        "Approve named exceptions"
    } else if lower.contains("authorize out-of-session pre-publication history repair") {
        "Authorize history repair"
    } else {
        return format!(
            "{code} · {}",
            detail
                .split_whitespace()
                .take(9)
                .collect::<Vec<_>>()
                .join(" ")
        );
    };
    format!("{code} · {short}")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn long_external_report_becomes_two_steps_and_four_choices() {
        let report = "## Waiting for user action\n\nRESULT: BLOCKED. Detailed verification prose.\n\n### Next action(s)\n\n- Adjudicator: pick exactly ONE in L.4 - (a) ratify the effective base 990112d with the realized table, (b) reissue the corrected footprint predicate, (c) sanction explicit exemptions for off-table rows, or (d) authorize out-of-session pre-publication history repair - and fill the VERDICT block; expected result: gate closes.\n- Operator: walk L1, L3 and L4 on the display workstation with PACKET_HOME in scratch; expected result: exhibits.\n- Packet (application): commit and reconcile after both actions.\n\nFull report: report.json";
        let plan = parse(report).unwrap();
        assert_eq!(
            plan.summary,
            "Implementation is paused for a decision and desktop checks."
        );
        assert_eq!(plan.steps.len(), 2);
        assert_eq!(
            plan.choices
                .iter()
                .map(|choice| choice.code)
                .collect::<Vec<_>>(),
            ['a', 'b', 'c', 'd']
        );
        assert_eq!(plan.choices[0].label, "a · Ratify effective base");
        assert!(plan.choices[3].detail.contains("history repair"));
        assert!(!plan.steps.join(" ").contains("expected result"));
    }
}
