pub(super) fn feature_phase_label(phase: crate::core::lifecycle::FeaturePhase) -> &'static str {
    use crate::core::lifecycle::FeaturePhase;
    match phase {
        FeaturePhase::Exploring => "Exploring",
        FeaturePhase::AwaitingAnswers => "Awaiting answers",
        FeaturePhase::AwaitingPlanConfirmation => "Awaiting plan confirmation",
        FeaturePhase::PlanReviewPending => "Independent plan review",
        FeaturePhase::PlanRework => "Plan rework",
        FeaturePhase::ReadyToDecompose => "Ready to generate tasks",
        FeaturePhase::Decomposing => "Generating tasks",
        FeaturePhase::Executing => "In implementation",
        FeaturePhase::Complete => "Complete",
        FeaturePhase::Cancelled => "Cancelled",
    }
}
