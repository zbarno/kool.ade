use super::{ChangeMetadata, ChangeStatus, replace_metadata};
use crate::domain::PlanComparison;

impl ChangeMetadata {
    pub fn adopt_typed_plan(
        markdown: &str,
        comparison: &PlanComparison,
        plan_id: &str,
        adr_path: Option<&str>,
    ) -> anyhow::Result<String> {
        crate::core::validation::validate_persisted(comparison)?;
        anyhow::ensure!(
            comparison.alternatives.len() == 2
                && comparison.alternatives[0].id == "A"
                && comparison.alternatives[1].id == "B"
                && ["A", "B"].contains(&comparison.recommendation.plan_id.as_str()),
            "Malformed typed plan comparison"
        );
        anyhow::ensure!(["A", "B"].contains(&plan_id), "Unknown plan alternative");
        let mut metadata = Self::require_markdown(markdown)?;
        anyhow::ensure!(
            metadata.schema_version == 2,
            "Feature predates the Compare Plans gate"
        );
        anyhow::ensure!(
            metadata.status == ChangeStatus::Ready,
            "Only a Ready feature can adopt a plan"
        );
        anyhow::ensure!(
            metadata.selected_alt.is_none(),
            "An adopted plan is frozen; start a new feature revision"
        );
        let selected = comparison
            .alternatives
            .iter()
            .find(|plan| plan.id == plan_id)
            .ok_or_else(|| anyhow::anyhow!("Selected plan is missing from the comparison"))?;
        // Workflow state owns adoption lifecycle and selected alternative;
        // keep the selected plan's complete specification in readable text.
        metadata.selected_alt = None;
        metadata.plan_comparison = None;
        metadata.comparison_history.clear();
        let mut updated = replace_metadata(markdown, &metadata)?;
        updated = remove_h2_section(&updated, "Plan Comparison")?;
        updated = remove_h2_section(&updated, "Plan Comparison History")?;
        let selected_content = render_selected(plan_id, selected);
        let selected_content = adr_path.map_or(selected_content.clone(), |path| {
            format!("{selected_content}\n\n**Decision record:** `{path}`")
        });
        replace_h2_section(&updated, "Selected Plan", &selected_content)
    }
}

fn remove_h2_section(markdown: &str, heading: &str) -> anyhow::Result<String> {
    let title = format!("## {heading}");
    let lines = markdown.lines().collect::<Vec<_>>();
    let starts = lines
        .iter()
        .enumerate()
        .filter_map(|(index, line)| (*line == title).then_some(index))
        .collect::<Vec<_>>();
    anyhow::ensure!(starts.len() <= 1, "Feature has duplicate {title} sections");
    let Some(start) = starts.first().copied() else {
        return Ok(markdown.to_owned());
    };
    let end = lines
        .iter()
        .enumerate()
        .skip(start + 1)
        .find_map(|(index, line)| line.starts_with("## ").then_some(index))
        .unwrap_or(lines.len());
    let mut updated = [lines[..start].join("\n"), lines[end..].join("\n")]
        .into_iter()
        .filter(|part| !part.is_empty())
        .collect::<Vec<_>>()
        .join("\n\n");
    if markdown.ends_with('\n') {
        updated.push('\n');
    }
    Ok(updated)
}

fn render_selected(id: &str, plan: &crate::domain::PlanAlternative) -> String {
    let mut output = format!(
        "**Alternative:** {id}\n\n{}\n\n### Phases\n",
        plan.objective
    );
    for (index, phase) in plan.phases.iter().enumerate() {
        output.push_str(&format!("\n{}. {}", index + 1, phase.name));
        for task in &phase.subtasks {
            output.push_str(&format!("\n   - {task}"));
        }
    }
    for (heading, entries) in [
        ("Files and subsystems", &plan.files_touched),
        ("Data and state changes", &plan.state_changes),
        ("Failure modes", &plan.failure_modes),
        ("Known risks", &plan.known_risks),
    ] {
        output.push_str(&format!("\n\n### {heading}\n"));
        for entry in entries {
            output.push_str(&format!("\n- {entry}"));
        }
    }
    output.push_str(&format!("\n\n**Effort:** {}\n", plan.effort_band));
    output.push_str(&format!("\n**Reversibility:** {}\n", plan.reversibility));
    output
}

fn replace_h2_section(markdown: &str, heading: &str, content: &str) -> anyhow::Result<String> {
    let title = format!("## {heading}");
    let lines = markdown.lines().collect::<Vec<_>>();
    let starts = lines
        .iter()
        .enumerate()
        .filter_map(|(index, line)| (*line == title).then_some(index))
        .collect::<Vec<_>>();
    anyhow::ensure!(starts.len() <= 1, "Feature has duplicate {title} sections");
    let block = format!("{title}\n\n{}", content.trim());
    let mut updated = if let Some(start) = starts.first().copied() {
        let end = lines
            .iter()
            .enumerate()
            .skip(start + 1)
            .find_map(|(index, line)| line.starts_with("## ").then_some(index))
            .unwrap_or(lines.len());
        [lines[..start].join("\n"), block, lines[end..].join("\n")]
            .into_iter()
            .filter(|part| !part.is_empty())
            .collect::<Vec<_>>()
            .join("\n\n")
    } else {
        format!("{}\n\n{block}", markdown.trim_end())
    };
    if markdown.ends_with('\n') {
        updated.push('\n');
    }
    Ok(updated)
}
