use crate::core::workflow::{TaskBatch, TaskStory};

pub fn slug(text: &str) -> String {
    slug_with_limit(text, 56)
}

fn slug_with_limit(text: &str, limit: usize) -> String {
    let mut out = String::new();
    for c in text.chars() {
        let lower: String = c.to_lowercase().collect();
        if out.len() + lower.len() > limit {
            break;
        }
        if c.is_alphanumeric() {
            out.push_str(&lower);
        } else if !out.is_empty() && !out.ends_with('-') {
            out.push('-');
        }
    }
    let out = out.trim_end_matches('-');
    if out.is_empty() {
        "feature".into()
    } else if matches!(
        out,
        "con"
            | "prn"
            | "aux"
            | "nul"
            | "com1"
            | "com2"
            | "com3"
            | "com4"
            | "com5"
            | "com6"
            | "com7"
            | "com8"
            | "com9"
            | "lpt1"
            | "lpt2"
            | "lpt3"
            | "lpt4"
            | "lpt5"
            | "lpt6"
            | "lpt7"
            | "lpt8"
            | "lpt9"
    ) {
        format!("{out}-feature")
    } else {
        out.to_owned()
    }
}
pub(super) fn task_name(feature_id: Option<&str>, index: usize, story: &TaskStory) -> String {
    match feature_id {
        Some(id) => format!("{id}-TASK-{}.md", slug_with_limit(&story.title, 100)),
        None => format!("{:03}-{}.md", index + 1, slug_with_limit(&story.title, 100)),
    }
}

pub fn is_task_story_filename(name: &str) -> bool {
    name.ends_with(".md")
        && (name.starts_with(|c: char| c.is_ascii_digit())
            || name.split_once("-TASK-").is_some_and(|(id, title)| {
                crate::artifacts::product_docs::valid_feature_id(id) && title != ".md"
            }))
}

pub(super) fn batch_slug(batch: &TaskBatch) -> String {
    let name = batch
        .feature_id
        .as_deref()
        .map(|id| batch.brief.feature_name.replace(id, ""))
        .unwrap_or_else(|| batch.brief.feature_name.clone());
    let slug = slug(&name);
    batch
        .feature_id
        .as_deref()
        .map(|id| format!("{id}-{slug}"))
        .unwrap_or(slug)
}

pub(super) fn ensure_unique_task_names(names: &[String]) -> anyhow::Result<()> {
    let mut unique = std::collections::HashSet::new();
    anyhow::ensure!(
        names.iter().all(|name| unique.insert(name)),
        "Task titles produce duplicate task IDs; use distinct concise titles"
    );
    Ok(())
}

pub(super) fn list(out: &mut String, heading: &str, values: &[String], numbered: bool) {
    out.push_str(&format!("\n## {heading}\n\n"));
    for (i, value) in values.iter().enumerate() {
        out.push_str(&format!(
            "{} {}\n",
            if numbered {
                format!("{}.", i + 1)
            } else {
                "-".into()
            },
            value.trim()
        ));
    }
}

fn optional_list(out: &mut String, heading: &str, values: &[String]) {
    if !values.is_empty() {
        list(out, heading, values, false);
    }
}

pub(super) fn render(
    batch: &TaskBatch,
    index: usize,
    story: &TaskStory,
    names: &[String],
) -> String {
    let b = &batch.brief;
    let task_id = batch
        .feature_id
        .as_deref()
        .map(|id| format!("{id}-TASK-{}", slug_with_limit(&story.title, 100)))
        .unwrap_or_else(|| format!("{:03}", index + 1));
    let mut out = format!(
        "# {} — {}\n\nFeature: {}\n\nStatus: Individually validated; see batch index for generation status.\n\n## Problem this ticket solves and why\n\n{}\n\n## Ticket goal — what changes when done\n\n{}\n\n## User story\n\n{}\n\n## Purpose\n\n{}\n\n## Specification references\n\nSource: [Approved specification](specification.md)\n\n## Implementation context\n\n{}\n",
        task_id,
        story.title.trim(),
        b.feature_name,
        story.intent,
        story.goal,
        story.user_story,
        story.purpose,
        story.context
    );
    if let Some(id) = &batch.feature_id {
        out.push_str(&format!("\nFeature ID: {id}\n"));
    }
    out.push_str(&format!(
        "Repository: {}\n",
        if story.target_repository.is_empty() {
            "root"
        } else {
            &story.target_repository
        }
    ));
    optional_list(
        &mut out,
        "Technical design and contracts",
        &story.technical_design,
    );
    out.push_str("\n## Approved scope mapping\n");
    for r in &story.scope_items {
        out.push_str(&format!("\n- Scope {r}: {}\n", b.in_scope[r - 1]));
    }
    for r in &story.success_criteria {
        out.push_str(&format!(
            "- Success criterion {r}: {}\n",
            b.success_criteria[r - 1]
        ));
    }
    out.push_str("\n## Dependencies\n\n");
    if story.dependencies.is_empty() {
        out.push_str("None. This task can start independently.\n");
    }
    for d in &story.dependencies {
        if batch.feature_id.is_some() {
            out.push_str(&format!(
                "- [{}]({}) must be complete.\n",
                names[d - 1].trim_end_matches(".md"),
                names[d - 1]
            ));
        } else {
            out.push_str(&format!(
                "- [Task {d:03}]({}) must be complete.\n",
                names[d - 1]
            ));
        }
    }
    list(
        &mut out,
        "Affected files and components",
        &story.affected_files,
        false,
    );
    list(
        &mut out,
        "Implementation steps",
        &story.implementation_steps,
        true,
    );
    list(
        &mut out,
        "Acceptance criteria",
        &story.acceptance_criteria,
        false,
    );
    list(&mut out, "Test plan", &story.test_plan, true);
    list(
        &mut out,
        "Verification commands and expected evidence",
        &story.verification_commands,
        true,
    );
    optional_list(
        &mut out,
        "Edge cases and failure handling",
        &story.edge_cases,
    );
    list(&mut out, "Constraints", &b.constraints, false);
    list(&mut out, "Out of scope", &b.out_of_scope, false);
    if !story.rollout_notes.trim().is_empty() {
        out.push_str(&format!(
            "\n## Rollout and compatibility\n\n{}\n",
            story.rollout_notes.trim()
        ));
    }
    list(
        &mut out,
        "Definition of done",
        &story.definition_of_done,
        false,
    );
    out.push_str("\n## Instructions for the implementing model\n\nRead this story and its linked dependencies before editing. Confirm the named existing files and interfaces; proposed files are explicitly labeled. Implement only this task's scope, preserving the constraints above. Run the verification commands and record actual results, changed files, and any remaining limitations. Do not mark complete on a build alone when acceptance criteria require runtime interaction. If an assumption contradicts the repository or approved specification, report the discrepancy rather than silently inventing a requirement.\n");
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::workflow::InterviewBrief;

    #[test]
    fn generated_story_omits_unneeded_optional_sections() {
        let brief = InterviewBrief {
            feature_name: "Small fix".into(),
            ..Default::default()
        };
        let batch = TaskBatch {
            brief,
            specification: String::new(),
            feature_id: None,
            contract: None,
            branch_targets: None,
            task_routing: Default::default(),
            stories: Vec::new(),
        };
        let story = TaskStory {
            title: "Fix crash".into(),
            intent: "Opening this view currently crashes.".into(),
            goal: "Open the view without crashing.".into(),
            user_story: "As a user I can open the view safely.".into(),
            purpose: "Restore the expected navigation behavior.".into(),
            context: "The failure occurs while opening this view.".into(),
            ..Default::default()
        };
        let rendered = render(&batch, 0, &story, &[]);
        assert!(!rendered.contains("## Technical design and contracts"));
        assert!(!rendered.contains("## Edge cases and failure handling"));
        assert!(!rendered.contains("## Rollout and compatibility"));
    }
}
