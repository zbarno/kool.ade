mod opportunity;
mod planner;
mod review;
mod runtime_qa;
mod task_graph;
mod validation;
mod work;

pub use opportunity::{
    ManagerActionKind, ManagerActionV1, ManagerOutputV1, ManagerOutputV2, OpportunityChoice,
    OpportunityConfidence, OpportunityDecisionV1, OpportunityKind, OpportunityOrigin,
    OpportunityProposalV1, OpportunityProposalV1Legacy, OpportunityScope, OpportunityTarget,
    validate_user_opportunity_decision,
};
pub use planner::{
    AcceptanceTestRefV1, ClarificationCategory, ClarificationOptionV1, ClarificationPriority,
    ClarificationProposalV1, PlannerOutputV1, PlannerOutputV2, PlannerQuestionV1, ReadinessV1,
    ReadinessV2, TechnicalResolutionV1,
};
pub use review::{
    CoverageStatus, CoverageV1, FindingKind, ReviewFindingV1, ReviewVerdictV1, Severity,
};
pub use runtime_qa::QaScenarioDraftV1;
pub use task_graph::{SubtaskDraftV1, TaskDraftV1, TaskGraphV1};
pub use validation::{
    OutputValidationContext, RoleOutputV1, validate_output, validate_output_with_context,
    validate_output_with_manifest,
};
pub use work::{
    AcceptanceCriterionV1, ApprovalEffect, BlockerDisposition, DeltaClassification, HumanChoiceV1,
    PrProposalV1, SpecChangeV1, SpecDeltaV1, WorkResultV1, WorkStatus,
};
