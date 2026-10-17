use crate::harness::prompt_catalog::{EvidenceConfidentiality, EvidenceItem, EvidenceOrigin};

pub(super) fn for_feature(
    state: &crate::core::state::PlannerState,
    feature_uid: &str,
    workspace_uid: &str,
) -> EvidenceItem {
    let overview =
        crate::core::repo_overview::scan_for_store(&state.repo_root, &state.planning_store);
    let mut content = String::new();
    if let Some(readme) = overview.readme {
        content.push_str("README excerpt:\n");
        content.push_str(&readme);
        content.push_str("\n\n");
    }
    content.push_str("Root manifests:\n");
    content.push_str(&overview.manifests.join("\n"));
    content.push_str("\n\nShallow repository tree:\n");
    content.push_str(&overview.tree_lines.join("\n"));
    if content.trim().is_empty() {
        content.push_str("No repository overview evidence was available.");
    }
    EvidenceItem {
        id: format!("repository-overview:{feature_uid}"),
        workspace_uid: workspace_uid.to_owned(),
        origin: EvidenceOrigin::Application,
        confidentiality: EvidenceConfidentiality::WorkspacePrivate,
        source: "read-only repository README, root manifests, and shallow file tree".into(),
        content: crate::core::context_build::clip(&content, 12 * 1024),
    }
}

#[cfg(test)]
#[path = "repository_evidence/tests.rs"]
mod tests;
