use super::{Work, WorkKind, WorkStatus};

impl PartialEq for Work {
    fn eq(&self, other: &Self) -> bool {
        self.uid == other.uid
            && self.key == other.key
            && self.kind == other.kind
            && self.title == other.title
            && self.original_goal == other.original_goal
            && self.request == other.request
            && self.status == other.status
            && self.feature_id == other.feature_id
            && self.feature_uid == other.feature_uid
            && self.parent_uid == other.parent_uid
            && self.task_intent_uid == other.task_intent_uid
            && self.task_intent_history == other.task_intent_history
            && self.task_decisions == other.task_decisions
            && self.task_intent_cycles == other.task_intent_cycles
            && self.task_intent_cycle_uid == other.task_intent_cycle_uid
            && self.task_intent_archived_at_ms == other.task_intent_archived_at_ms
            && self.git_intent == other.git_intent
            && self.git_intents == other.git_intents
            && self.source_branch == other.source_branch
            && self.destination_branch == other.destination_branch
            && self.routing_overrides == other.routing_overrides
            && self.routing_inherited_from == other.routing_inherited_from
            && self.follow_up_task == other.follow_up_task
            && self.code_review_disposition == other.code_review_disposition
            && self.operational_recovery == other.operational_recovery
            && self.detail == other.detail
    }
}

impl Eq for Work {}

impl Work {
    pub fn task_intent_root_uid(&self) -> Option<&str> {
        self.task_intent_uid.as_deref().or_else(|| {
            (self.parent_uid.is_none() && self.kind.starts_task_intent())
                .then_some(self.uid.as_str())
        })
    }

    /// Link work only when its parent or immutable feature identity identifies
    /// one unambiguous owning root.
    pub fn inherit_task_intent(&mut self, existing: &[Work]) {
        if self.parent_uid.is_none() && self.kind.starts_task_intent() {
            self.task_intent_uid.get_or_insert_with(|| self.uid.clone());
            return;
        }
        let parent = self.parent_uid.as_deref().and_then(|parent_uid| {
            existing
                .iter()
                .find(|candidate| candidate.uid == parent_uid)
        });
        let parent_root = parent.and_then(Work::task_intent_root_uid);
        let feature_root = self.feature_uid.as_deref().and_then(|feature_uid| {
            let mut roots = existing.iter().filter(|candidate| {
                candidate.parent_uid.is_none()
                    && candidate.kind.starts_task_intent()
                    && candidate.feature_uid.as_deref() == Some(feature_uid)
            });
            let root = roots.next()?;
            roots.next().is_none().then_some(root.uid.as_str())
        });
        if self.task_intent_uid.is_none() {
            if let (Some(parent), Some(feature)) = (parent_root, feature_root)
                && parent != feature
            {
                return;
            }
            self.task_intent_uid = parent_root.or(feature_root).map(str::to_owned);
        }
        if self.task_intent_cycle_uid.is_some() {
            return;
        }
        self.task_intent_cycle_uid = parent
            .and_then(|parent| parent.task_intent_cycle_uid.clone())
            .or_else(|| {
                let root_uid = self.task_intent_uid.as_deref()?;
                existing
                    .iter()
                    .find(|candidate| candidate.uid == root_uid)
                    .and_then(|root| root.task_intent_cycle_uid.clone())
            });
    }

    pub fn task_intent_uid_for_feature(existing: &[Work], feature_uid: &str) -> Option<String> {
        let mut roots = existing.iter().filter(|candidate| {
            candidate.parent_uid.is_none()
                && candidate.kind.starts_task_intent()
                && candidate.feature_uid.as_deref() == Some(feature_uid)
        });
        let root = roots.next()?;
        roots
            .next()
            .is_none()
            .then(|| root.task_intent_root_uid().map(str::to_owned))
            .flatten()
    }

    pub fn new(key: String, title: String, request: String, detail: String) -> Self {
        let original_goal = Some(request.clone());
        Self {
            uid: uuid::Uuid::new_v4().hyphenated().to_string(),
            key,
            kind: WorkKind::Feature,
            title,
            original_goal,
            request,
            status: WorkStatus::InProgress,
            feature_id: None,
            feature_uid: None,
            parent_uid: None,
            task_intent_uid: None,
            task_intent_history: Vec::new(),
            task_decisions: Vec::new(),
            task_intent_cycles: Vec::new(),
            task_intent_cycle_uid: None,
            task_intent_archived_at_ms: None,
            git_intent: None,
            git_intents: std::collections::BTreeMap::new(),
            source_branch: None,
            destination_branch: None,
            routing_overrides: std::collections::BTreeMap::new(),
            routing_inherited_from: None,
            follow_up_task: None,
            code_review_disposition: None,
            operational_recovery: None,
            plan_confirmation: None,
            detail,
            record_revision: 0,
            record_baseline: None,
        }
    }

    pub fn board_column(&self) -> usize {
        self.status.board_column()
    }
}

pub fn normalize_task_intents(work: &mut [Work]) {
    for item in work.iter_mut() {
        if item.parent_uid.is_none() && item.kind.starts_task_intent() {
            item.task_intent_uid.get_or_insert_with(|| item.uid.clone());
            item.original_goal
                .get_or_insert_with(|| item.request.clone());
            super::cycles::ensure_initial_cycle(item);
        }
    }
    let snapshot = work.to_vec();
    for item in work.iter_mut() {
        item.inherit_task_intent(&snapshot);
    }
}
