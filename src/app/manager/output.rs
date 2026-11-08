use super::catalog;
use crate::app::session;

pub struct ManagerOutput {
    pub summary: String,
    pub proposed_actions: Vec<crate::harness::prompt_catalog::ManagerActionV1>,
    pub opportunities: Vec<crate::harness::prompt_catalog::OpportunityProposalV1>,
    pub evidence_hashes: std::collections::HashMap<String, String>,
    pub evidence_confidentialities:
        std::collections::HashMap<String, crate::harness::prompt_catalog::EvidenceConfidentiality>,
    pub planning_revision: String,
    pub code_review_targets:
        std::collections::HashMap<String, catalog::code_reviews::FindingTarget>,
}

pub fn persist_output(project: &session::Project, output: &ManagerOutput) -> String {
    let messages = catalog::persist_opportunities(project, output);
    if messages.is_empty() {
        return output.summary.clone();
    }
    format!("{}\n\n{}", output.summary, messages.join("\n"))
}
