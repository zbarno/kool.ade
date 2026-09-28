use std::path::{Path, PathBuf};

use crate::{
    artifacts::layout::ArtifactLayout,
    domain::{PlanAlternative, PlanComparison},
};

pub(crate) fn prepare_plan_choice_record(
    repo: &Path,
    feature_id: &str,
    feature_uid: &str,
    comparison: &PlanComparison,
    selected_alt: &str,
) -> anyhow::Result<(String, String)> {
    anyhow::ensure!(["A", "B"].contains(&selected_alt), "Unknown selected plan");
    anyhow::ensure!(
        comparison.alternatives.len() == 2,
        "Plan comparison must contain two alternatives"
    );
    let selected = comparison
        .alternatives
        .iter()
        .find(|plan| plan.id == selected_alt)
        .ok_or_else(|| anyhow::anyhow!("Selected plan is missing"))?;
    let title = format!("Adopt Alt {selected_alt} for {feature_id}");
    let layout = ArtifactLayout::new(repo);
    let directory = layout.decisions_root();
    ensure_safe_path(repo, &directory)?;
    let identity = super::identity::new_adr_identity(&directory, &title, Some(feature_uid));
    let body = render(
        &identity.display_id,
        feature_id,
        comparison,
        selected_alt,
        selected,
    );
    let content = super::identity::embed(&body, None, &identity)?;
    let name = format!(
        "{}-{}.md",
        identity.display_id,
        crate::artifacts::task_docs::slug(&title)
    );
    let path = layout
        .decision_record(&name)
        .ok_or_else(|| anyhow::anyhow!("Generated plan ADR path is unsafe"))?;
    ensure_safe_path(repo, &path)?;
    anyhow::ensure!(
        !path.exists(),
        "Plan ADR path already exists: {}",
        path.display()
    );
    Ok((
        path.strip_prefix(repo)?
            .to_string_lossy()
            .replace('\\', "/"),
        content,
    ))
}

fn render(
    id: &str,
    feature: &str,
    comparison: &PlanComparison,
    selected: &str,
    chosen: &PlanAlternative,
) -> String {
    let mut body = format!(
        "# {id}: Adopt Alt {selected} for {feature}\n\nStatus: Accepted\nRelated change: `{feature}`\nOrigin: Packet authored both alternatives under operator-approved Option A (CLR-029).\nRecommendation: Alt {} — {}\n\n## Decision\n\n**Alt {selected}: {}**\n\n{}\n\n## Alternatives considered\n",
        comparison.recommendation.plan_id,
        comparison.recommendation.rationale,
        chosen.objective,
        chosen.effort_band,
    );
    for plan in &comparison.alternatives {
        body.push_str(&format!("\n### Alt {}\n\n{}\n", plan.id, plan.objective));
        let phases = plan
            .phases
            .iter()
            .map(|phase| format!("{}: {}", phase.name, phase.subtasks.join("; ")))
            .collect::<Vec<_>>();
        add_list(&mut body, "Phases", &phases);
        add_list(&mut body, "Files touched", &plan.files_touched);
        add_list(&mut body, "State changes", &plan.state_changes);
        add_list(&mut body, "Failure modes", &plan.failure_modes);
        add_list(&mut body, "Known risks", &plan.known_risks);
        body.push_str(&format!("\nReversibility: {}\n", plan.reversibility));
    }
    let axes = variation_axes(&comparison.alternatives[0], &comparison.alternatives[1]);
    add_list(&mut body, "Variation axes", &axes);
    add_list(
        &mut body,
        "Recommendation evidence",
        &comparison.recommendation.evidence,
    );
    body
}

fn variation_axes(a: &PlanAlternative, b: &PlanAlternative) -> Vec<String> {
    let mut axes = Vec::new();
    if a.phases != b.phases {
        axes.push("Phase granularity and sequencing".into());
    }
    if a.files_touched != b.files_touched {
        axes.push("Subsystem and file boundaries".into());
    }
    if a.state_changes != b.state_changes {
        axes.push("Data and state shape".into());
    }
    if a.failure_modes != b.failure_modes || a.reversibility != b.reversibility {
        axes.push("Failure handling and rollback posture".into());
    }
    if a.known_risks != b.known_risks {
        axes.push("Risk posture".into());
    }
    axes
}

fn add_list(body: &mut String, heading: &str, values: &[String]) {
    if values.is_empty() {
        return;
    }
    body.push_str(&format!("\n## {heading}\n"));
    for value in values {
        body.push_str(&format!("\n- {value}"));
    }
    body.push('\n');
}

fn ensure_safe_path(repo: &Path, target: &Path) -> anyhow::Result<()> {
    let relative = target.strip_prefix(repo)?;
    let mut path = PathBuf::from(repo);
    for component in relative.components() {
        anyhow::ensure!(
            matches!(component, std::path::Component::Normal(_)),
            "Unsafe ADR path"
        );
        path.push(component);
        if let Ok(metadata) = std::fs::symlink_metadata(&path) {
            anyhow::ensure!(
                !metadata.file_type().is_symlink(),
                "Symlink in ADR path: {}",
                path.display()
            );
            if path != target {
                anyhow::ensure!(metadata.is_dir(), "ADR parent is not a directory");
            }
        }
    }
    Ok(())
}
