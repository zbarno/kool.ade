use super::{ChangeMetadata, ChangeStatus, replace_metadata};
use crate::domain::{PlanAlternative, PlanComparison};

impl ChangeMetadata {
    pub fn save_plan_comparison(
        markdown: &str,
        comparison: PlanComparison,
    ) -> anyhow::Result<String> {
        validate_pair(&comparison)?;
        anyhow::ensure!(
            comparison.selected_plan.is_none(),
            "A generated comparison cannot preselect an alternative"
        );
        let mut metadata = Self::require_markdown(markdown)?;
        anyhow::ensure!(
            metadata.schema_version == 2,
            "Feature predates the Compare Plans gate"
        );
        anyhow::ensure!(
            metadata.status == ChangeStatus::Ready,
            "Only a Ready feature can store plan alternatives"
        );
        anyhow::ensure!(
            metadata.selected_alt.is_none(),
            "Discard the adopted plan before comparing again"
        );
        if let Some(previous) = metadata.plan_comparison.take() {
            metadata.comparison_history.push(previous);
        }
        metadata.plan_comparison = Some(comparison);
        let mut updated = replace_metadata(markdown, &metadata)?;
        updated = replace_h2_section(
            &updated,
            "Plan Comparison",
            &render_comparison(metadata.plan_comparison.as_ref().unwrap()),
        )?;
        if !metadata.comparison_history.is_empty() {
            let history = metadata
                .comparison_history
                .iter()
                .enumerate()
                .map(|(index, item)| {
                    format!(
                        "### Previous comparison {}\n\n{}",
                        index + 1,
                        render_comparison(item)
                    )
                })
                .collect::<Vec<_>>()
                .join("\n\n");
            updated = replace_h2_section(&updated, "Plan Comparison History", &history)?;
        }
        Ok(updated)
    }

    pub fn select_plan(markdown: &str, plan_id: &str) -> anyhow::Result<String> {
        Self::adopt_plan(markdown, plan_id, None)
    }

    pub fn adopt_plan(
        markdown: &str,
        plan_id: &str,
        adr_path: Option<&str>,
    ) -> anyhow::Result<String> {
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
            "An adopted plan is frozen; discard before comparing again"
        );
        let comparison = metadata
            .plan_comparison
            .as_mut()
            .ok_or_else(|| anyhow::anyhow!("Feature has no generated plan comparison"))?;
        let selected = comparison
            .alternatives
            .iter()
            .find(|plan| plan.id == plan_id)
            .cloned()
            .ok_or_else(|| anyhow::anyhow!("Selected plan is missing from the comparison"))?;
        let comparison_snapshot = comparison.clone();
        comparison.selected_plan = Some(plan_id.to_owned());
        metadata.selected_alt = Some(plan_id.to_owned());
        let updated = replace_metadata(markdown, &metadata)?;
        let updated = replace_h2_section(
            &updated,
            "Plan Comparison",
            &render_comparison(&comparison_snapshot),
        )?;
        let selected_content = render_selected(plan_id, &selected);
        let selected_content = adr_path.map_or(selected_content.clone(), |path| {
            format!("{selected_content}\n\n**Decision record:** `{path}`")
        });
        replace_h2_section(&updated, "Selected Plan", &selected_content)
    }

    pub fn discard_plan_comparison(markdown: &str) -> anyhow::Result<String> {
        let mut metadata = Self::require_markdown(markdown)?;
        anyhow::ensure!(
            metadata.schema_version == 2,
            "Feature predates the Compare Plans gate"
        );
        anyhow::ensure!(
            metadata.selected_alt.is_none(),
            "An adopted plan cannot be discarded; start a new feature revision"
        );
        if let Some(previous) = metadata.plan_comparison.take() {
            metadata.comparison_history.push(previous);
        }
        let updated = replace_metadata(markdown, &metadata)?;
        let mut updated = remove_h2_section(&updated, "Plan Comparison")?;
        updated = remove_h2_section(&updated, "Selected Plan")?;
        if !metadata.comparison_history.is_empty() {
            let history = metadata
                .comparison_history
                .iter()
                .enumerate()
                .map(|(index, item)| {
                    format!(
                        "### Previous comparison {}\n\n{}",
                        index + 1,
                        render_comparison(item)
                    )
                })
                .collect::<Vec<_>>()
                .join("\n\n");
            updated = replace_h2_section(&updated, "Plan Comparison History", &history)?;
        }
        Ok(updated)
    }
}

fn validate_pair(comparison: &PlanComparison) -> anyhow::Result<()> {
    anyhow::ensure!(
        comparison.alternatives.len() == 2,
        "Plan comparison requires exactly two alternatives"
    );
    anyhow::ensure!(
        comparison.alternatives[0].id == "A" && comparison.alternatives[1].id == "B",
        "Plan comparison IDs must be A and B"
    );
    anyhow::ensure!(
        ["A", "B"].contains(&comparison.recommendation.plan_id.as_str()),
        "Plan recommendation must select A or B"
    );
    Ok(())
}

fn render_comparison(comparison: &PlanComparison) -> String {
    let mut output = format!(
        "Kool.ad/e recommendation: **Plan {}** — {}\n\n",
        comparison.recommendation.plan_id, comparison.recommendation.rationale
    );
    for evidence in &comparison.recommendation.evidence {
        output.push_str(&format!("Evidence: {evidence}\n"));
    }
    for plan in &comparison.alternatives {
        output.push_str(&format!("\n{}", render_selected(&plan.id, plan)));
    }
    output
}

fn render_selected(id: &str, plan: &PlanAlternative) -> String {
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
    render_list(&mut output, "Files and subsystems", &plan.files_touched);
    render_list(&mut output, "Data and state changes", &plan.state_changes);
    render_list(&mut output, "Failure modes", &plan.failure_modes);
    output.push_str(&format!("\n**Effort:** {}\n", plan.effort_band));
    render_list(&mut output, "Known risks", &plan.known_risks);
    output.push_str(&format!("\n**Reversibility:** {}\n", plan.reversibility));
    output
}

fn render_list(output: &mut String, title: &str, entries: &[String]) {
    output.push_str(&format!("\n\n### {title}\n"));
    for entry in entries {
        output.push_str(&format!("\n- {entry}"));
    }
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
    if let Some(start) = starts.first().copied() {
        let end = lines
            .iter()
            .enumerate()
            .skip(start + 1)
            .find_map(|(index, line)| line.starts_with("## ").then_some(index))
            .unwrap_or(lines.len());
        let mut next = lines[..start].join("\n");
        if !next.is_empty() {
            next.push_str("\n\n");
        }
        next.push_str(&block);
        if end < lines.len() {
            next.push_str("\n\n");
            next.push_str(&lines[end..].join("\n"));
        }
        if markdown.ends_with('\n') {
            next.push('\n');
        }
        Ok(next)
    } else {
        let mut next = markdown.trim_end().to_owned();
        next.push_str("\n\n");
        next.push_str(&block);
        if markdown.ends_with('\n') {
            next.push('\n');
        }
        Ok(next)
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
    let mut next = lines[..start].join("\n");
    if end < lines.len() {
        if !next.is_empty() {
            next.push_str("\n\n");
        }
        next.push_str(&lines[end..].join("\n"));
    }
    if markdown.ends_with('\n') {
        next.push('\n');
    }
    Ok(next)
}
