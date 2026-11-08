use crate::core::repository_knowledge::{ProfileSource, RepositoryKnowledgeProfileV1};

/// App-selected identity and the existing bounded profile for one repository.
pub struct InvestigationContext {
    pub source: ProfileSource,
    pub profile: RepositoryKnowledgeProfileV1,
    pub workspace_id: String,
    pub task_id: String,
    pub cancellation_id: String,
    pub task_revision: String,
    pub comparison: Option<InvestigationRevisionContext>,
}

pub struct InvestigationRevisionContext {
    pub source: ProfileSource,
    pub profile: RepositoryKnowledgeProfileV1,
}
