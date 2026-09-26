//! Structural gate for full specification replacements. Semantic truth and
//! decision preservation remain authoring obligations, not parser guarantees.
use pulldown_cmark::{Event, HeadingLevel, Parser, Tag, TagEnd};

pub const SECTIONS: [&str; 6] = [
    "Overview",
    "Users and Outcomes",
    "Current Capabilities",
    "Architecture and Constraints",
    "Decisions",
    "Quality and Acceptance",
];

pub const LEGACY_SECTIONS: [&str; 13] = [
    "1. Vision",
    "2. Scope",
    "3. Actors and Roles",
    "4. Feature Inventory",
    "5. Functional Requirements",
    "6. Non-Functional Requirements",
    "7. Data Model",
    "8. Architecture",
    "9. Environment, Launch, and Preconditions",
    "10. Decisions Log",
    "11. Risks and Open Concerns",
    "12. Acceptance / Definition of Done",
    "13. Source Map",
];

pub fn validate_layout(markdown: &str) -> Result<(), String> {
    let mut headings = Vec::new();
    let mut active = None;
    for event in Parser::new(markdown) {
        match event {
            Event::Start(Tag::Heading { level, .. }) => active = Some((level, String::new())),
            Event::Text(text) | Event::Code(text) => {
                if let Some((_, title)) = &mut active {
                    title.push_str(&text);
                }
            }
            Event::End(TagEnd::Heading(_)) => {
                if let Some(heading) = active.take() {
                    headings.push(heading);
                }
            }
            _ => {}
        }
    }
    let titles: Vec<_> = headings
        .iter()
        .filter(|(l, _)| *l == HeadingLevel::H1)
        .collect();
    let suffix = " — Living Technical Specification";
    if titles.len() != 1
        || !titles[0].1.ends_with(suffix)
        || titles[0].1.trim_end_matches(suffix).trim().is_empty()
        || headings.first() != titles.first().copied()
    {
        return Err("updated_specification must begin with exactly one H1: <Project Name> — Living Technical Specification".into());
    }
    let sections: Vec<_> = headings
        .iter()
        .filter(|(l, _)| *l == HeadingLevel::H2)
        .map(|(_, title)| title.as_str())
        .collect();
    let has_core = SECTIONS
        .iter()
        .all(|required| sections.iter().filter(|title| **title == *required).count() == 1);
    if !has_core && sections.as_slice() != LEGACY_SECTIONS {
        return Err(format!(
            "updated_specification must include the six core H2 concepts ({}), or retain the recognized legacy layout; optional H2 sections may be added. Return the complete document.",
            SECTIONS.join("; ")
        ));
    }
    Ok(())
}

pub fn validate_feature(id: &str, markdown: &str) -> anyhow::Result<()> {
    let mut headings = Vec::new();
    let mut current = None;
    for event in Parser::new(markdown) {
        match event {
            Event::Start(Tag::Heading { level, .. }) => current = Some((level, String::new())),
            Event::Text(text) | Event::Code(text) => {
                if let Some((_, title)) = &mut current {
                    title.push_str(&text);
                }
            }
            Event::End(TagEnd::Heading(_)) => {
                if let Some(head) = current.take() {
                    headings.push(head);
                }
            }
            _ => {}
        }
    }
    let titles = headings
        .iter()
        .filter(|(level, _)| *level == HeadingLevel::H1)
        .collect::<Vec<_>>();
    anyhow::ensure!(
        titles.len() == 1 && titles[0].1.starts_with(&format!("{id}: ")),
        "feature requires exactly one matching H1"
    );
    let sections = headings
        .iter()
        .filter(|(level, _)| *level == HeadingLevel::H2)
        .map(|(_, title)| title.as_str())
        .collect::<Vec<_>>();
    anyhow::ensure!(
        sections
            == [
                "Intent",
                "Current Behavior",
                "Desired Behavior",
                "Scope",
                "Affected Product Areas",
                "Requirements",
                "Decisions and Assumptions",
                "Acceptance Criteria"
            ],
        "feature requires the eight ordered sections"
    );
    anyhow::ensure!(
        [
            "Draft",
            "Ready",
            "Implementing",
            "Reconciliation",
            "Implemented",
            "Abandoned"
        ]
        .iter()
        .any(|status| markdown
            .lines()
            .any(|line| line.starts_with(&format!("**Status:** {status}")))),
        "feature requires a recognized status"
    );
    Ok(())
}

#[cfg(test)]
pub(crate) fn fixture(body: &str) -> String {
    let mut text = "# Fixture — Living Technical Specification\n\nVersion: 1.0. Status: test fixture. Authority: test scenario.\n\n**Maintenance.** Test harness supplies complete revisions; git preserves history.\n\n".to_string();
    for section in SECTIONS {
        text.push_str(&format!("## {section}\n\n{body}\n\n"));
    }
    text
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn current_specification_obeys_layout() {
        let repo = std::env::temp_dir().join(format!(
            "packet_spec_layout_{}-{}",
            std::process::id(),
            chrono::Utc::now().timestamp_nanos_opt().unwrap()
        ));
        std::fs::create_dir_all(&repo).unwrap();
        crate::artifacts::migration::bootstrap_product(&repo, "Layout Test").unwrap();
        let modules = crate::artifacts::product_docs::load_modules(&repo)
            .unwrap()
            .expect("bootstrap creates the modular product layout");
        for body in &modules {
            crate::artifacts::product_docs::validate_module(body).unwrap();
        }
        let _ = std::fs::remove_dir_all(repo);
    }

    #[test]
    fn accepts_subsections_and_ignores_fenced_example_headings() {
        let text = fixture(
            "Unknown pending confirmation.\n\n### Detail\n\n```markdown\n# Example\n## Example section\n```",
        );
        validate_layout(&text).unwrap();
    }

    #[test]
    fn requires_core_concepts_and_allows_project_specific_sections() {
        let text = fixture("Unknown pending confirmation.");
        validate_layout(&format!("{text}\n## Billing\n\nProject-specific policy.\n")).unwrap();
        for invalid in [
            text.replace("## Users and Outcomes", "### Users and Outcomes"),
            text.replace("## Users and Outcomes", "## Current Capabilities"),
            format!("{text}\n# Another title\n"),
            text.replace("Fixture — Living Technical Specification", "Fixture"),
        ] {
            assert!(validate_layout(&invalid).is_err());
        }
    }
}
