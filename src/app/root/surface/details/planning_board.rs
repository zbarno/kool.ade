use super::*;

impl KooladeApp {
    pub(in crate::app::root::surface) fn planning_board_for_surface(
        &self,
    ) -> crate::ui::planning_board::ViewModel {
        match &self.screen {
            Screen::Connected(p) => {
                let mut planning_work =
                    crate::core::planning_work::cards(&p.state, &p.planning_work);
                if let Some(active_key) = p
                    .active_turn
                    .as_ref()
                    .and(p.active_planning_work.as_deref())
                    && let Some(work) = planning_work.iter_mut().find(|work| work.key == active_key)
                {
                    work.status = crate::core::planning_work::WorkStatus::InProgress;
                }
                let mut planning_items = p
                    .state
                    .items
                    .iter()
                    .chain(&self.synth)
                    .chain(&p.state.resolved_items)
                    .cloned()
                    .collect::<Vec<_>>();
                planning_items.sort_by_key(|item| (item.priority.rank(), item.id.clone()));
                planning_items.dedup_by(|a, b| a.id == b.id);
                let eligible_item_ids = crate::core::routing::eligible_items(
                    &planning_items,
                    &self.cached_user,
                    &p.state.config.stakeholders,
                )
                .into_iter()
                .map(|item| item.id.clone())
                .collect();
                let (clarification_questions, clarification_error) =
                    match crate::core::planning::QuestionStore::new(&p.state.planning_store).list()
                    {
                        Ok(questions) => (questions, None),
                        Err(error) => (Vec::new(), Some(error.to_string())),
                    };
                let (opportunities, mut opportunity_error) =
                    match crate::core::opportunity::OpportunityStore::new(&p.state.planning_store)
                        .list_with_revisions()
                    {
                        Ok(proposals) => (proposals, None),
                        Err(error) => (Vec::new(), Some(error.to_string())),
                    };
                let opportunity_decisions =
                    match crate::core::opportunity::OpportunityStore::new(&p.state.planning_store)
                        .list_decisions()
                    {
                        Ok(decisions) => decisions
                            .into_iter()
                            .map(|decision| (decision.opportunity_uid.clone(), decision))
                            .collect(),
                        Err(error) => {
                            opportunity_error.get_or_insert_with(|| error.to_string());
                            std::collections::BTreeMap::new()
                        }
                    };
                let opportunity_discovery_settings =
                    match crate::core::opportunity::OpportunityStore::new(&p.state.planning_store)
                        .discovery_settings()
                    {
                        Ok(settings) => settings,
                        Err(error) => {
                            opportunity_error.get_or_insert_with(|| error.to_string());
                            crate::core::opportunity::OpportunityDiscoverySettingsV1::default()
                        }
                    };
                let self_improvement_runs = opportunities
                    .iter()
                    .filter(|(proposal, _)| {
                        proposal.target.kind
                            == crate::core::opportunity::OpportunityTargetKindV1::KooladeProduct
                    })
                    .filter_map(|(proposal, _)| {
                        let decision = opportunity_decisions.get(&proposal.opportunity_uid)?;
                        if decision.decision
                            != crate::core::opportunity::OpportunityDecisionKindV1::Pursue
                        {
                            return None;
                        }
                        let run_uid = crate::core::self_improvement::SelfImprovementStore::run_uid_for_decision(
                            &decision.uid,
                        )
                        .ok()?;
                        crate::core::self_improvement::SelfImprovementStore::new(
                            &p.state.planning_store,
                        )
                        .load(&run_uid)
                        .ok()
                        .map(|run| (proposal.opportunity_uid.clone(), run))
                    })
                    .collect();
                let lifecycle_store =
                    crate::core::lifecycle::LifecycleStore::new(&p.state.planning_store);
                let (lifecycle_feature_tasks, task_error) = match lifecycle_store.list_tasks() {
                    Ok(tasks) => (tasks, None),
                    Err(error) => (Vec::new(), Some(error.to_string())),
                };
                let (lifecycle_tasks, mut lifecycle_error) = match task_error {
                    Some(error) => (std::collections::BTreeMap::new(), Some(error)),
                    None => super::lifecycle_ui::board_phases(p, &lifecycle_feature_tasks),
                };
                let lifecycle_features = match lifecycle_store.list_features() {
                    Ok(features) => features,
                    Err(error) => {
                        lifecycle_error.get_or_insert_with(|| error.to_string());
                        Vec::new()
                    }
                };
                let mut lifecycle_pr_statuses = std::collections::BTreeMap::new();
                let mut lifecycle_pr_work = Vec::new();
                for task in &lifecycle_feature_tasks {
                    if task.parent_task_uid.is_some() {
                        continue;
                    }
                    let (Some(planning_uid), Some(pr_uid)) = (
                        task.planning_item_uid.as_deref(),
                        task.pr_work_item_uid.as_deref(),
                    ) else {
                        continue;
                    };
                    let Some(document) = p.task_documents.iter().find(|document| {
                        document
                            .identity
                            .as_ref()
                            .is_some_and(|identity| identity.uid == planning_uid)
                    }) else {
                        continue;
                    };
                    match lifecycle_store.load_pr_preparation(pr_uid) {
                        Ok(preparation) => {
                            lifecycle_pr_statuses.insert(document.path.clone(), preparation.status);
                            lifecycle_pr_work.push(
                                crate::ui::planning_board::LifecyclePrBoardTask {
                                    parent_path: document.path.clone(),
                                    parent_title: document.title.clone(),
                                    preparation,
                                },
                            );
                        }
                        Err(error) => {
                            lifecycle_error.get_or_insert_with(|| error.to_string());
                        }
                    }
                }
                crate::ui::planning_board::ViewModel {
                    task_documents: p.task_documents.clone(),
                    planning_work,
                    planning_items,
                    clarification_questions,
                    clarification_error,
                    opportunities,
                    opportunity_decisions,
                    self_improvement_runs,
                    opportunity_error,
                    opportunity_discovery_settings,
                    setup_attention: self.setup_attention.clone(),
                    setup_checking: self.setup_probe.is_some(),
                    eligible_item_ids,
                    archived: p.archived_tasks.clone(),
                    cancelled: p.cancelled_work.clone(),
                    lifecycle_tasks,
                    lifecycle_pr_statuses,
                    lifecycle_pr_work,
                    lifecycle_features,
                    lifecycle_feature_tasks,
                    lifecycle_error,
                }
            }
            _ => crate::ui::planning_board::ViewModel::default(),
        }
    }
}
