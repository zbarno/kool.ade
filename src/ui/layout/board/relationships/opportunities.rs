use std::collections::{HashMap, HashSet};

pub(super) type OpportunityRelations =
    HashMap<String, Vec<crate::ui::task_detail::OpportunityRelationView>>;

pub fn opportunity_relations(board: &crate::ui::planning_board::ViewModel) -> OpportunityRelations {
    let mut counts = HashMap::<String, usize>::new();
    for (proposal, _) in &board.opportunities {
        *counts.entry(proposal.opportunity_uid.clone()).or_default() += 1;
    }
    let mut relations = OpportunityRelations::new();
    for (proposal, _) in &board.opportunities {
        let navigable = counts.get(&proposal.opportunity_uid) == Some(&1)
            && uuid::Uuid::parse_str(&proposal.opportunity_uid).is_ok();
        for feature_uid in &proposal.related_feature_uids {
            insert_relation(
                &mut relations,
                feature_uid,
                proposal,
                "Explicit feature relation",
                navigable,
            );
        }
        for task_uid in &proposal.related_task_uids {
            insert_relation(
                &mut relations,
                task_uid,
                proposal,
                "Explicit task relation",
                navigable,
            );
        }
    }
    for related in relations.values_mut() {
        let mut seen = HashSet::new();
        related.retain(|item| seen.insert((item.opportunity_uid.clone(), item.relation.clone())));
    }
    relations
}

fn insert_relation(
    relations: &mut OpportunityRelations,
    related_uid: &str,
    proposal: &crate::core::opportunity::OpportunityV1,
    relation: &str,
    navigable: bool,
) {
    relations.entry(related_uid.to_owned()).or_default().push(
        crate::ui::task_detail::OpportunityRelationView {
            opportunity_uid: proposal.opportunity_uid.clone(),
            conversation_key: crate::core::task_conversation::opportunity_conversation_key(
                &proposal.opportunity_uid,
            ),
            title: proposal.problem_or_opportunity.clone(),
            kind: kind_label(&proposal.kind).to_owned(),
            status: status_label(&proposal.status).to_owned(),
            relation: relation.to_owned(),
            navigable,
        },
    );
}

fn kind_label(kind: &crate::core::opportunity::OpportunityKindV1) -> &'static str {
    use crate::core::opportunity::OpportunityKindV1 as Kind;
    match kind {
        Kind::NewFeature => "New feature",
        Kind::ExistingFeatureExtension => "Feature extension",
        Kind::BugFix => "Bug fix",
        Kind::Integration => "Integration",
        Kind::TechStackSupport => "Technology support",
        Kind::Quality => "Quality",
        Kind::Security => "Security",
        Kind::Performance => "Performance",
        Kind::DeveloperExperience => "Developer experience",
        Kind::Documentation => "Documentation",
        Kind::Infrastructure => "Infrastructure",
    }
}

fn status_label(status: &crate::core::opportunity::OpportunityStatusV1) -> &'static str {
    use crate::core::opportunity::OpportunityStatusV1 as Status;
    match status {
        Status::Draft => "Draft",
        Status::Proposed => "Proposed",
        Status::AwaitingDecision => "Awaiting decision",
        Status::InformationRequested => "Information requested",
        Status::ApprovedToPlan => "Approved to plan",
        Status::Declined => "Declined",
        Status::Deferred => "Deferred",
        Status::Superseded => "Superseded",
        Status::Planning => "Planning",
        Status::Implemented => "Implemented",
    }
}
