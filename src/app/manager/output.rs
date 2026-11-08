use super::catalog;
use crate::app::session;

pub struct ManagerOutput {
    pub summary: String,
    pub uncertainty_reported: bool,
    pub proposed_actions: Vec<crate::harness::prompt_catalog::ManagerActionV1>,
    pub opportunities: Vec<crate::harness::prompt_catalog::OpportunityProposalV1>,
    pub evidence_hashes: std::collections::HashMap<String, String>,
    pub evidence_confidentialities:
        std::collections::HashMap<String, crate::harness::prompt_catalog::EvidenceConfidentiality>,
    pub planning_revision: String,
    pub code_review_targets:
        std::collections::HashMap<String, catalog::code_reviews::FindingTarget>,
    pub verified_repository_evidence: Vec<crate::domain::chatlog::VerifiedRepositoryEvidence>,
    pub baseline_review_id: Option<String>,
}

pub fn persist_output(project: &session::Project, output: &ManagerOutput) -> String {
    let messages = catalog::persist_opportunities(project, output);
    if messages.is_empty() {
        return output.summary.clone();
    }
    format!("{}\n\n{}", output.summary, messages.join("\n"))
}

pub(super) fn uncertainty_reported(value: &str) -> bool {
    let normalized = value.trim().to_ascii_lowercase();
    let normalized = normalized.trim_end_matches(['.', '!', '?']).trim();
    !normalized.is_empty()
        && !matches!(
            normalized,
            "none"
                | "none known"
                | "no uncertainty"
                | "no known uncertainty"
                | "no uncertainties"
                | "not applicable"
                | "n/a"
                | "nil"
        )
}

#[cfg(test)]
mod uncertainty_tests {
    use super::uncertainty_reported;

    #[test]
    fn uncertainty_telemetry_ignores_empty_and_explicitly_clear_answers() {
        assert!(!uncertainty_reported(""));
        assert!(!uncertainty_reported("No known uncertainty."));
    }

    #[test]
    fn uncertainty_telemetry_counts_a_stated_open_question() {
        assert!(uncertainty_reported(
            "The deployment state has not been observed."
        ));
    }
}
