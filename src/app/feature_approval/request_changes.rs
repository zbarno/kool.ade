use super::super::{KooladeApp, Screen};

impl KooladeApp {
    pub(crate) fn request_feature_changes(&mut self, feature_id: &str, candidate_hash: &str) {
        let (work_key, findings) = match request_work_key(&self.screen, feature_id, candidate_hash)
        {
            Ok(key) => key,
            Err(error) => {
                self.toasts.warning(error);
                return;
            }
        };
        let request = format!(
            "[KOOLADE_PLAN_REMEDIATION feature={feature_id} snapshot_hash={candidate_hash}] The user requested changes to the current plan candidate for {feature_id} (candidate hash {candidate_hash}). Full persisted independent-review outcome and required plan_remediation identity: {findings}. The Manager must account for every persisted finding exactly once and return typed plan_remediation decisions: RepairPlan for technical or clarity fixes, AskUser when product intent is unclear, PromoteFollowUp for distinct nonblocking scope with a matching follow_up_task offer, or ContestFinding with evidence_refs selected from the persisted reviewer evidence. Ask what specific change the user wants when required, and create the matching board question or follow-up task for those actions. Update the living feature specification only through the normal planning flow. Preserve the original goal and do not treat this candidate as approved; a changed candidate needs explicit confirmation and fresh independent review."
        );
        self.start_turn_for_work(
            &request,
            crate::core::workflow::TurnPurpose::PlanRemediation,
            None,
            Some(work_key),
        );
    }
}

fn request_work_key(
    screen: &Screen,
    feature_id: &str,
    candidate_hash: &str,
) -> Result<(String, String), String> {
    let Screen::Connected(project) = screen else {
        return Err("Connect a project before requesting plan changes.".into());
    };
    if project.active_turn.is_some() {
        return Err("Wait for the current planning reply before requesting plan changes.".into());
    }
    let (_, markdown) = project
        .state
        .active_features
        .iter()
        .find(|(id, _)| id == feature_id)
        .ok_or_else(|| format!("Cannot request changes: {feature_id} is no longer active."))?;
    let identity = crate::domain::ArtifactIdentity::from_markdown(markdown)
        .map_err(|error| format!("Cannot request changes: feature identity is invalid ({error})."))?
        .ok_or_else(|| format!("Cannot request changes: {feature_id} has no feature identity."))?;
    let snapshot = crate::core::lifecycle::plan_snapshot::from_feature_with_author(
        &project.state.planning_store,
        feature_id,
        &identity,
        markdown,
        &crate::core::workflow::feature_contract(markdown),
    )
    .map_err(|error| format!("Cannot request changes: current candidate is invalid ({error})."))?;
    if snapshot.content_hash != candidate_hash {
        return Err(
            "This plan candidate changed. Reopen its confirmation card before requesting changes."
                .into(),
        );
    }
    let work_key = project
        .planning_work
        .iter()
        .find(|work| {
            work.kind == crate::core::planning_work::WorkKind::Feature
                && work.feature_id.as_deref() == Some(feature_id)
                && work.feature_uid.as_deref() == Some(identity.uid.as_str())
        })
        .map(|work| work.key.clone())
        .ok_or_else(|| {
            String::from(
                "Cannot route this request: the feature's original planning work is unavailable.",
            )
        })?;
    let review_record = crate::core::lifecycle::plan_snapshot::plan_review_verdict(
        &project.state.planning_store,
        &snapshot,
    )
    .map_err(|error| error.to_string())?
    .map(|verdict| {
        let receipt = crate::core::lifecycle::plan_snapshot::plan_review_receipt(
            &project.state.planning_store,
            &snapshot,
        )
        .map_err(|error| error.to_string())?
        .ok_or_else(|| "persisted review verdict has no matching receipt".to_owned())?;
        serde_json::to_string(&serde_json::json!({
            "feature_uid": snapshot.feature_uid,
            "snapshot_uid": snapshot.uid,
            "snapshot_hash": snapshot.content_hash,
            "review_receipt_uid": receipt.receipt_uid,
            "verdict": verdict,
            "evidence_refs": receipt.evidence_refs,
            "subject_hash": receipt.subject_sha256,
        }))
        .map_err(|error| error.to_string())
    })
    .transpose()?
    .unwrap_or_else(|| "no persisted review verdict".into());
    Ok((work_key, review_record))
}
