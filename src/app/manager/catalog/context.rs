use crate::{app::session::Project, harness::prompt_catalog::EvidenceItem};

mod decisions;
mod published;
mod ranking;

pub(super) use decisions::validated_decisions;
#[cfg(test)]
pub(super) use published::published_candidates;
pub(super) use published::published_spec_evidence;
#[cfg(test)]
pub(super) use ranking::terms;
pub(super) use ranking::{resolve_sources, telemetry};

pub(super) fn authoritative_evidence(
    project: &Project,
    workspace_uid: &str,
    goal: &str,
) -> Vec<EvidenceItem> {
    let mut evidence = published_spec_evidence(project, workspace_uid, goal);
    evidence.extend(validated_decisions(project, workspace_uid));
    evidence
}

#[cfg(test)]
#[path = "context_tests.rs"]
mod tests;
