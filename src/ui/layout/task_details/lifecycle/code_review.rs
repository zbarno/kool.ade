use crate::{ui::task_detail::LifecycleView, ui::theme};

pub(super) fn paint(
    ui: &mut egui::Ui,
    mut surface: Option<&mut dyn crate::ui::Surface>,
    lifecycle: &LifecycleView,
) {
    if let Some(error) = &lifecycle.code_review_load_error {
        ui.group(|ui| {
            ui.label(theme::section_heading("Code review evidence unavailable"));
            ui.colored_label(theme::WARNING, error);
        });
    }
    egui::CollapsingHeader::new(format!(
        "Independent code review · {} attempts",
        lifecycle.code_review_attempts.len()
    ))
    .default_open(true)
    .show(ui, |ui| {
        for (index, attempt) in lifecycle.code_review_attempts.iter().enumerate() {
            ui.small(format!(
                "Attempt {} · {:?} · prompt {} · reviewer run {}",
                index + 1,
                attempt.decision,
                attempt.prompt_sha256.as_deref().unwrap_or("not recorded"),
                attempt.reviewer_run_id.as_deref().unwrap_or("not recorded"),
            ));
        }
        if !lifecycle.code_review_dispositions.is_empty() {
            ui.label("Manager finding dispositions");
            for disposition in &lifecycle.code_review_dispositions {
                ui.label(format!(
                    "{} · {} scope · {:?} · {:?} · {} occurrences · {} · work {}",
                    disposition.finding_id,
                    finding_scope_label(disposition.scope),
                    disposition.status,
                    disposition.work_status,
                    disposition.occurrence_count,
                    disposition.title,
                    disposition.work_id
                ));
            }
        }
        let Some(review) = &lifecycle.current_code_review else {
            ui.label("No independent code review receipt matches the current verified task head.");
            return;
        };
        ui.label(format!(
            "Current result · {}",
            if review.verdict.pass {
                "pass"
            } else {
                "findings require Manager disposition"
            }
        ));
        ui.small(format!(
            "Task {} · subject {} · diff {} · head {}",
            review.task_uid, review.subject_hash, review.diff_hash, review.head_sha
        ));
        ui.small(format!(
            "Reviewer prompt {} · run {} · session {}",
            review.prompt_hash, review.reviewer_run_id, review.reviewer_session_id
        ));
        ui.label("Coverage");
        ui.label(format!(
            "Examined files · {}",
            if review.verdict.examined_files.is_empty() {
                "none".into()
            } else {
                review.verdict.examined_files.join(", ")
            }
        ));
        ui.label(format!(
            "Tested evidence · {}",
            if review.verdict.tested_evidence_ids.is_empty() {
                "none".into()
            } else {
                review.verdict.tested_evidence_ids.join(", ")
            }
        ));
        for area in &review.verdict.unexamined_areas {
            ui.colored_label(theme::WARNING, format!("Unexamined: {area}"));
        }
        for coverage in &review.verdict.coverage {
            ui.label(format!(
                "{} · {:?} · evidence {}",
                coverage.requirement,
                coverage.status,
                coverage.evidence.join(", ")
            ));
        }
        for untested in &review.verdict.untested {
            ui.colored_label(theme::WARNING, format!("Untested: {untested}"));
        }
        if review.verdict.findings.is_empty() {
            ui.label("No findings on the current reviewed subject.");
        }
        for finding in &review.verdict.findings {
            ui.group(|ui| {
                ui.label(theme::section_heading(format!(
                    "{} · {:?} · {} · {:?}",
                    finding.id,
                    finding.severity,
                    finding_scope_label(finding.scope),
                    finding.kind
                )));
                ui.label(format!("Requirement · {}", finding.requirement));
                ui.label(format!(
                    "Affected requirements · {}",
                    finding.affected_requirement_ids.join(", ")
                ));
                ui.label(format!(
                    "Location · {} · {:?}–{:?}",
                    finding
                        .location_detail
                        .as_ref()
                        .map(|location| location.file.as_str())
                        .unwrap_or(&finding.location),
                    finding
                        .location_detail
                        .as_ref()
                        .and_then(|location| location.line_start),
                    finding
                        .location_detail
                        .as_ref()
                        .and_then(|location| location.line_end)
                ));
                ui.label(format!("Evidence · {}", finding.evidence.join(", ")));
                ui.label(format!("Suggested action · {:?}", finding.suggested_action));
                ui.label(format!("Rationale · {}", finding.rationale));
                if let Some(repro) = &finding.repro {
                    ui.label(format!("Reproduction · {repro}"));
                }
                ui.label(format!(
                    "Smallest correction · {}",
                    finding.proposed_correction
                ));
                if let Some(disposition) =
                    lifecycle
                        .code_review_dispositions
                        .iter()
                        .find(|disposition| {
                            disposition.finding_id == finding.id
                                && disposition.subject_hash == review.subject_hash
                        })
                {
                    ui.label(format!(
                        "Manager disposition · {:?} · {:?} · {} occurrences · {} · work {}",
                        disposition.status,
                        disposition.work_status,
                        disposition.occurrence_count,
                        disposition.title,
                        disposition.work_id
                    ));
                    if disposition.status
                        == crate::core::planning_work::CodeReviewDispositionStatusV1::Open
                        && disposition.scope
                            == Some(crate::harness::prompt_catalog::FindingScopeV1::NonBlocking)
                        && review.verdict.pass
                        && disposition.subject_hash == review.subject_hash
                        && ui.button("Accept this residual risk").clicked()
                        && let Some(surface) = surface.as_deref_mut()
                    {
                        surface.dispatch(crate::ui::ApplicationCommand::ApproveCodeReviewRisk {
                            work_id: disposition.work_id.clone(),
                            task_uid: review.task_uid.clone(),
                            finding_id: disposition.finding_id.clone(),
                            subject_hash: disposition.subject_hash.clone(),
                        });
                    }
                } else {
                    ui.colored_label(theme::WARNING, "Manager disposition not recorded");
                }
            });
        }
    });
}

fn finding_scope_label(
    scope: Option<crate::harness::prompt_catalog::FindingScopeV1>,
) -> &'static str {
    match scope {
        Some(crate::harness::prompt_catalog::FindingScopeV1::Blocking) => "Blocking",
        Some(crate::harness::prompt_catalog::FindingScopeV1::NonBlocking) => "Non-blocking",
        None => "Scope unavailable",
    }
}
