use crate::ui::{Surface, theme};

mod feature_delta;
mod trace_navigation;

pub(super) fn paint(ui: &mut egui::Ui, surface: &mut dyn Surface, markdown: &str) {
    match surface.feature_specification_status(markdown) {
        Ok(Some(status)) => {
            ui.horizontal(|ui| {
                ui.label(egui::RichText::new("Feature status").weak());
                ui.strong(
                    status
                        .delivery_stage
                        .map(crate::ui::FeatureDeliveryStage::label)
                        .unwrap_or("Status not recorded"),
                );
            });
            if let Some(navigation) = surface.specification_trace_navigation()
                && status.feature_uid.as_deref() == Some(navigation.feature_uid.as_str())
            {
                trace_navigation::paint(ui, surface, navigation);
            }
            egui::CollapsingHeader::new("Technical details")
                .default_open(false)
                .show(ui, |ui| {
                    row(
                        ui,
                        "Current rendered product-spec hash",
                        status.current_spec_hash.as_deref(),
                    );
                    ui.label(format!("Drift status · {}", status.drift_status));
                    row(
                        ui,
                        "Approved target",
                        status.approved_target_hash.as_deref(),
                    );
                    row(ui, "Deployed specification hash", status.deployed_spec_hash.as_deref());
                    row(ui, "Latest production deployment status", status.deployment_status.as_deref());
                    if let Some(observed_at) = status.deployment_observed_at_ms {
                        let observed = chrono::DateTime::from_timestamp_millis(observed_at as i64)
                            .map_or_else(|| observed_at.to_string(), |time| time.to_rfc3339());
                        ui.small(format!("Deployment status observed · {observed}"));
                    }
                    ui.label(format!(
                        "Revision {} · {} · {} pending triggers",
                        status
                            .revision
                            .map_or_else(|| "not recorded".into(), |revision| revision.to_string()),
                        status
                            .reconciliation_status
                            .map_or("not recorded", status_label),
                        status.pending_triggers
                    ));
                    if let Some(reason) = status.outcome_reason.as_deref() {
                        ui.label(egui::RichText::new(reason).color(theme::WARNING));
                    }
                    for (task, head) in status.reviewed_heads {
                        ui.label(format!("Reviewed implementation · {task} · {head}"));
                    }
                    for (task, commit) in status.merged_commits {
                        ui.label(format!(
                            "Provider-reported merge commit · {task} · {commit}"
                        ));
                    }
                    if let Some(current) = status.current_implementation {
                        let freshness = if status.current_implementation_stale {
                            "stale · registered checkout has changed or is unavailable"
                        } else {
                            "verified against current registered checkout"
                        };
                        ui.label(format!(
                            "Current implementation · {freshness} · task {} · commit {} · checkout {} · specification revision {}",
                            current.task_uid, current.merge_commit_sha,
                            current.checked_out_head_sha, current.specification_revision
                        ));
                    }
                    for delta in status.feature_deltas {
                        if let Some(command) = feature_delta::paint(
                            ui,
                            surface,
                            status.feature_uid.as_deref(),
                            status.revision,
                            delta,
                        ) {
                            surface.dispatch(command);
                        }
                    }
                });
        }
        Ok(None) => {}
        Err(error) => {
            ui.colored_label(
                theme::WARNING,
                format!("Specification lifecycle status unavailable: {error}"),
            );
        }
    }
}

pub(super) fn paint_product_summary(
    ui: &mut egui::Ui,
    surface: &dyn Surface,
    features: &[(String, String)],
) {
    ui.heading("Current product");
    match surface.product_specification_status() {
        Ok(Some(aggregate)) => {
            ui.label(format!(
                "Baseline status · {} · {} lifecycle features · {} open findings",
                aggregate_status(aggregate.status),
                aggregate.lifecycle_features.len(),
                aggregate.findings.len()
            ));
            egui::CollapsingHeader::new("Technical details")
                .default_open(false)
                .show(ui, |ui| {
                    ui.label(format!(
                        "Product specification hash · {} · {} pending triggers",
                        &aggregate.rendered_product_hash[..12],
                        aggregate.pending_trigger_count
                    ));
                });
            if !aggregate.findings.is_empty() {
                ui.colored_label(
                    theme::WARNING,
                    "Needs review · product changes are unresolved",
                );
            }
            for finding in aggregate.findings {
                ui.label(format!(
                    "{} · {} · {:?} · {} · trigger {}",
                    finding.feature_uid,
                    finding.requirement_id,
                    finding.classification,
                    if finding.needs_user_reapproval {
                        "approval required"
                    } else {
                        "nonblocking"
                    },
                    finding.event_id
                ));
                for decision in &finding.decision_refs {
                    ui.small(format!("  Decision: {decision}"));
                }
                for task in &finding.affected_task_ids {
                    ui.small(format!("  Task: {task}"));
                }
                for reference in finding.test_refs.iter().chain(&finding.evidence_refs) {
                    ui.small(format!("  Trace: {reference}"));
                }
            }
        }
        Ok(None) => {
            ui.label("Baseline status not recorded");
        }
        Err(error) => {
            ui.colored_label(
                theme::WARNING,
                format!("Product reconciliation unavailable: {error}"),
            );
        }
    }
    let mut recorded = 0;
    let mut pending = 0;
    let mut deployed = 0;
    egui::CollapsingHeader::new("Feature delivery status")
        .default_open(true)
        .show(ui, |ui| {
            for (id, markdown) in features {
                match surface.feature_specification_status(markdown) {
                    Ok(Some(status)) => {
                        recorded += 1;
                        pending += status.pending_triggers;
                        deployed += usize::from(status.deployed_spec_hash.is_some());
                        ui.horizontal_wrapped(|ui| {
                            ui.label(id.as_str());
                            ui.strong(
                                status
                                    .delivery_stage
                                    .map(crate::ui::FeatureDeliveryStage::label)
                                    .unwrap_or("Status not recorded"),
                            );
                        });
                        egui::CollapsingHeader::new(format!("{id} · supporting evidence"))
                            .default_open(false)
                            .show(ui, |ui| {
                                ui.label(format!(
                                    "Drift · {} · product revision {} · reconciliation {} · {} pending triggers",
                                    status.drift_status,
                                    status.revision.map_or_else(|| "not recorded".into(), |revision| revision.to_string()),
                                    status.reconciliation_status.map_or("not recorded", status_label),
                                    status.pending_triggers
                                ));
                                if let Some(hash) = status.approved_target_hash.as_deref() {
                                    ui.small(format!("Approved plan · {}", short_hash(Some(hash))));
                                }
                                for (task, head) in &status.reviewed_heads {
                                    ui.small(format!("Verified task {task} · {head}"));
                                }
                                for (task, commit) in &status.merged_commits {
                                    ui.small(format!("Merged task {task} · {commit}"));
                                }
                                if let Some(hash) = status.deployed_spec_hash.as_deref() {
                                    ui.small(format!("Deployment · {}", short_hash(Some(hash))));
                                } else {
                                    ui.small(format!("Deployment · {}", status.deployment_status.as_deref().unwrap_or("not recorded")));
                                }
                            });
                    }
                    Ok(None) => { ui.label(format!("{id} · lifecycle state not recorded")); }
                    Err(error) => { ui.colored_label(theme::WARNING, format!("{id} · lifecycle state unavailable: {error}")); }
                };
            }
            ui.label(format!(
                "{} lifecycle-backed features · {pending} pending triggers · {deployed} with verified production deployment",
                recorded
            ));
        });
}

fn aggregate_status(status: crate::core::lifecycle::ProductSpecAggregateStatusV1) -> &'static str {
    use crate::core::lifecycle::ProductSpecAggregateStatusV1 as Status;
    match status {
        Status::Synchronized => "synchronized",
        Status::FindingsRequireReview => "findings require review",
        Status::PendingTriggers => "pending triggers",
        Status::DriftDetected => "product drift detected",
        Status::BaselineIncomplete => "baseline incomplete",
    }
}

fn short_hash(hash: Option<&str>) -> String {
    hash.map(|hash| hash.chars().take(12).collect())
        .unwrap_or_else(|| "not recorded".into())
}

fn row(ui: &mut egui::Ui, label: &str, value: Option<&str>) {
    ui.horizontal_wrapped(|ui| {
        ui.label(egui::RichText::new(label).weak());
        ui.label(value.unwrap_or("Not recorded"));
    });
}

fn status_label(status: crate::core::lifecycle::SpecReconciliationStatusV1) -> &'static str {
    match status {
        crate::core::lifecycle::SpecReconciliationStatusV1::Synchronized => "synchronized",
        crate::core::lifecycle::SpecReconciliationStatusV1::OutstandingNonblockingFindings => {
            "open findings"
        }
        crate::core::lifecycle::SpecReconciliationStatusV1::BlockingDrift => "blocking drift",
        crate::core::lifecycle::SpecReconciliationStatusV1::NeedsHumanDecision => "needs decision",
    }
}
