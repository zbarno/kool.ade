use super::Finding;

/// Attach stable prerequisite IDs and canonicalize their order.
pub(super) fn requires(
    mut finding: Finding,
    prerequisites: impl IntoIterator<Item = String>,
) -> Finding {
    finding.prerequisites = prerequisites.into_iter().collect();
    finding.prerequisites.sort();
    finding.prerequisites.dedup();
    finding
}

pub(super) fn docker_engine() -> String {
    "setup:docker:engine".into()
}

pub(super) fn docker_capabilities() -> String {
    "setup:docker:capabilities".into()
}

pub(super) fn docker_rootless() -> String {
    "setup:docker:rootless".into()
}
