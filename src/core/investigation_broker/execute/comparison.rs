use super::{
    super::{
        helpers::{evidence_map, safe_line, stale_receipt},
        types::{
            InvestigationComparisonRefV1, InvestigationOutcomeV1, InvestigationReceiptV1,
            InvestigationRequestV1, InvestigationSourceRefV1,
        },
    },
    InvestigationContext,
};
use crate::core::repository_knowledge::read_pinned_file_bounded_with_deadline;
use crate::core::repository_knowledge::verify_profile_freshness_with_deadline;
use std::sync::atomic::{AtomicBool, Ordering};

pub(super) fn execute(
    context: &InvestigationContext,
    request: &InvestigationRequestV1,
    started: std::time::Instant,
    deadline: std::time::Instant,
    cancelled: &AtomicBool,
) -> Result<InvestigationReceiptV1, String> {
    let Some(comparison) = &context.comparison else {
        return Err("comparison revision is not app-authorized".into());
    };
    let primary = evidence_map(&context.profile)?;
    let other = evidence_map(&comparison.profile)?;
    let mut receipt = stale_receipt(request);
    receipt.inspected_scope.clear();
    receipt.omitted_scope.clear();
    receipt.uncertainty.clear();
    receipt.source_refs.clear();
    receipt.comparison_refs.clear();
    receipt.findings.clear();
    receipt.outcome = InvestigationOutcomeV1::NotFoundInScope;
    let mut bytes_read = 0usize;
    let mut files_read = 0u16;

    'paths: for (path_index, path) in request.scope.paths.iter().enumerate() {
        if cancelled.load(Ordering::Acquire) {
            receipt.outcome = InvestigationOutcomeV1::Cancelled;
            receipt.omitted_scope.push(path.clone());
            break;
        }
        if std::time::Instant::now() >= deadline {
            receipt.outcome = InvestigationOutcomeV1::InsufficientEvidence;
            receipt.omitted_scope.push(path.clone());
            receipt
                .uncertainty
                .push("The comparison time budget was exhausted.".into());
            break;
        }
        if receipt.source_refs.len().saturating_add(2) > usize::from(request.budget.max_files) {
            receipt.omitted_scope.push(path.clone());
            continue;
        }
        let (Some(left_ref), Some(right_ref)) = (primary.get(path), other.get(path)) else {
            receipt.omitted_scope.push(path.clone());
            continue;
        };
        if !request.allowed_evidence_classes.contains(&left_ref.kind)
            || !request.allowed_evidence_classes.contains(&right_ref.kind)
        {
            receipt.omitted_scope.push(path.clone());
            continue;
        }
        receipt.inspected_scope.push(path.clone());
        let remaining = usize::try_from(request.budget.max_bytes)
            .unwrap_or(usize::MAX)
            .saturating_sub(bytes_read);
        let left_result = read_pinned_file_bounded_with_deadline(
            &context.source,
            &context.profile.key,
            path,
            remaining,
            deadline,
            cancelled,
        );
        let (left, left_hash) = match left_result {
            Ok(value) => value,
            Err(error) => {
                return failed_read(receipt, path, error, started, bytes_read, files_read);
            }
        };
        files_read = files_read.saturating_add(1);
        bytes_read = bytes_read.saturating_add(left.len());
        let remaining = usize::try_from(request.budget.max_bytes)
            .unwrap_or(usize::MAX)
            .saturating_sub(bytes_read);
        let right_result = read_pinned_file_bounded_with_deadline(
            &comparison.source,
            &comparison.profile.key,
            path,
            remaining,
            deadline,
            cancelled,
        );
        let (right, right_hash) = match right_result {
            Ok(value) => value,
            Err(error) => {
                return failed_read(receipt, path, error, started, bytes_read, files_read);
            }
        };
        files_read = files_read.saturating_add(1);
        bytes_read = bytes_read.saturating_add(right.len());
        if left_hash != left_ref.content_sha256 || right_hash != right_ref.content_sha256 {
            receipt.outcome = InvestigationOutcomeV1::ConflictingEvidence;
            receipt.findings.clear();
            receipt.source_refs.clear();
            receipt.comparison_refs.clear();
            receipt.uncertainty.push(format!(
                "Pinned comparison hashes disagree with profile evidence for {path}."
            ));
            break;
        }
        receipt.source_refs.push(source_ref(
            request,
            "left",
            path,
            &context.profile.key.commit_sha,
            &left_hash,
        ));
        receipt.source_refs.push(source_ref(
            request,
            "right",
            path,
            &comparison.profile.key.commit_sha,
            &right_hash,
        ));
        receipt.comparison_refs.push(InvestigationComparisonRefV1 {
            evidence_id: format!(
                "investigation:{}:comparison:{path}",
                request.idempotency_key
            ),
            path: path.clone(),
            left_source_sha: context.profile.key.commit_sha.clone(),
            left_content_sha256: left_hash,
            right_source_sha: comparison.profile.key.commit_sha.clone(),
            right_content_sha256: right_hash,
        });
        let before = left.lines().collect::<Vec<_>>();
        let after = right.lines().collect::<Vec<_>>();
        let mut changed = 0usize;
        for index in 0..before.len().max(after.len()) {
            let old = before.get(index).copied().unwrap_or("[no line]");
            let new = after.get(index).copied().unwrap_or("[no line]");
            if old != new {
                changed += 1;
                receipt.findings.push(format!(
                    "Untrusted repository evidence at {path}:{}: comparison left {} ({}) right {} ({}).",
                    index + 1,
                    context.profile.key.commit_sha,
                    safe_line(old),
                    comparison.profile.key.commit_sha,
                    safe_line(new),
                ));
                if receipt.findings.len() >= usize::from(request.budget.max_matches) {
                    receipt
                        .omitted_scope
                        .push(format!("{path} (remaining changed lines)"));
                    receipt
                        .omitted_scope
                        .extend(request.scope.paths.iter().skip(path_index + 1).cloned());
                    break 'paths;
                }
            }
        }
        if changed > 0 {
            receipt.outcome = InvestigationOutcomeV1::Findings;
        }
    }
    if receipt.outcome == InvestigationOutcomeV1::NotFoundInScope {
        if receipt.source_refs.is_empty() {
            receipt.outcome = InvestigationOutcomeV1::InsufficientEvidence;
            receipt.uncertainty.push(
                "The requested comparison scope contained no verified evidence in both revisions."
                    .into(),
            );
        } else if !receipt.omitted_scope.is_empty() {
            receipt.outcome = InvestigationOutcomeV1::InsufficientEvidence;
            receipt.uncertainty.push(
                "Some comparison scope was omitted, so no-change cannot be established.".into(),
            );
        }
    }
    if receipt.findings.is_empty()
        && !receipt.source_refs.is_empty()
        && receipt.outcome == InvestigationOutcomeV1::NotFoundInScope
    {
        for path in &receipt.inspected_scope {
            receipt.findings.push(format!(
                "Untrusted repository evidence at {path}: inspected content was identical across the selected revisions."
            ));
        }
        receipt.outcome = InvestigationOutcomeV1::Findings;
    }
    if cancelled.load(Ordering::Acquire) || std::time::Instant::now() >= deadline {
        receipt.outcome = if cancelled.load(Ordering::Acquire) {
            InvestigationOutcomeV1::Cancelled
        } else {
            InvestigationOutcomeV1::InsufficientEvidence
        };
        receipt.source_refs.clear();
        receipt.comparison_refs.clear();
        receipt.findings.clear();
        receipt
            .uncertainty
            .push("Comparison ended before both source revisions could be revalidated.".into());
    } else if verify_profile_freshness_with_deadline(
        &context.source,
        &context.profile,
        deadline,
        cancelled,
    )
    .is_err()
        || verify_profile_freshness_with_deadline(
            &comparison.source,
            &comparison.profile,
            deadline,
            cancelled,
        )
        .is_err()
        || context.source.planning_store.revision().ok().as_deref()
            != Some(request.expected_task_revision.as_str())
    {
        receipt.outcome = InvestigationOutcomeV1::StaleSource;
        receipt.source_refs.clear();
        receipt.comparison_refs.clear();
        receipt.findings.clear();
        receipt.uncertainty.push(
            "A source or planning revision changed before the comparison was accepted.".into(),
        );
    }
    receipt.files_read = files_read;
    receipt.bytes_read = u32::try_from(bytes_read).unwrap_or(u32::MAX);
    receipt.elapsed_ms = u32::try_from(started.elapsed().as_millis()).unwrap_or(u32::MAX);
    receipt.confidence = match receipt.outcome {
        InvestigationOutcomeV1::Findings if receipt.omitted_scope.is_empty() => 75,
        InvestigationOutcomeV1::Findings => 55,
        InvestigationOutcomeV1::NotFoundInScope => 60,
        InvestigationOutcomeV1::InsufficientEvidence => 20,
        _ => 0,
    };
    Ok(receipt)
}

fn failed_read(
    mut receipt: InvestigationReceiptV1,
    path: &str,
    error: String,
    started: std::time::Instant,
    bytes_read: usize,
    files_read: u16,
) -> Result<InvestigationReceiptV1, String> {
    receipt.outcome = if error.contains("revision") {
        InvestigationOutcomeV1::StaleSource
    } else if error.contains("cancelled") {
        InvestigationOutcomeV1::Cancelled
    } else {
        InvestigationOutcomeV1::InsufficientEvidence
    };
    receipt.omitted_scope.push(path.to_owned());
    receipt.findings.clear();
    receipt.source_refs.clear();
    receipt.comparison_refs.clear();
    receipt.uncertainty.push(format!(
        "Comparison evidence could not be verified: {}",
        safe_line(&error)
    ));
    receipt.bytes_read = u32::try_from(bytes_read).unwrap_or(u32::MAX);
    receipt.files_read = files_read;
    receipt.elapsed_ms = u32::try_from(started.elapsed().as_millis()).unwrap_or(u32::MAX);
    receipt.confidence = 0;
    Ok(receipt)
}

fn source_ref(
    request: &InvestigationRequestV1,
    side: &str,
    path: &str,
    source_sha: &str,
    content_sha256: &str,
) -> InvestigationSourceRefV1 {
    InvestigationSourceRefV1 {
        evidence_id: format!("investigation:{}:{side}:{path}", request.idempotency_key),
        path: path.to_owned(),
        source_sha: source_sha.to_owned(),
        content_sha256: content_sha256.to_owned(),
    }
}
