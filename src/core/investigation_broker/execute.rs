mod validation;
use validation::{status_receipt, validate_context};

use super::{
    helpers::{evidence_map, fingerprint, safe_line, stale_receipt},
    types::{
        InvestigationOutcomeV1, InvestigationReceiptV1, InvestigationRequestV1,
        InvestigationSourceRefV1,
    },
};
use crate::core::repository_knowledge::{
    ProfileSource, RepositoryKnowledgeProfileV1, read_pinned_file_bounded_with_deadline,
    verify_profile_freshness_with_deadline,
};
use std::{
    collections::{BTreeMap, BTreeSet},
    sync::{
        Mutex,
        atomic::{AtomicBool, Ordering},
    },
};

/// App-selected identity and the existing bounded profile for one repository.
pub struct InvestigationContext {
    pub source: ProfileSource,
    pub profile: RepositoryKnowledgeProfileV1,
    pub workspace_id: String,
    pub task_id: String,
    pub cancellation_id: String,
    pub task_revision: String,
}

#[derive(Default)]
struct ReceiptCache {
    by_key: BTreeMap<String, (String, InvestigationReceiptV1)>,
}

/// Serializes identical/replayed requests and caps in-memory receipt retention.
#[derive(Default)]
pub struct InvestigationBroker {
    cache: Mutex<ReceiptCache>,
}

impl InvestigationBroker {
    pub fn investigate(
        &self,
        context: &InvestigationContext,
        request: &InvestigationRequestV1,
        cancelled: &AtomicBool,
    ) -> Result<InvestigationReceiptV1, String> {
        request.validate()?;
        let started = std::time::Instant::now();
        let deadline =
            started + std::time::Duration::from_millis(u64::from(request.budget.max_elapsed_ms));
        let fresh = match validate_context(context, request, deadline, cancelled) {
            Ok(fresh) => fresh,
            Err(error) if error.contains("cancelled") => {
                return Ok(status_receipt(
                    request,
                    InvestigationOutcomeV1::Cancelled,
                    error,
                ));
            }
            Err(error) if error.contains("wall-time budget") => {
                return Ok(status_receipt(
                    request,
                    InvestigationOutcomeV1::InsufficientEvidence,
                    error,
                ));
            }
            Err(error) => return Err(error),
        };
        if !fresh {
            return Ok(stale_receipt(request));
        }
        let fingerprint = fingerprint(request)?;
        let mut cache = self
            .cache
            .lock()
            .map_err(|_| "investigation broker is unavailable")?;
        if let Some((prior_fingerprint, receipt)) = cache.by_key.get(&request.idempotency_key) {
            if prior_fingerprint == &fingerprint {
                return Ok(receipt.clone());
            }
            return Err("idempotency key was replayed with a different request".into());
        }
        if cache.by_key.len() >= 8 {
            return Err("Manager investigation session exhausted its follow-on limit".into());
        }
        let receipt = execute(context, request, started, deadline, cancelled)?;
        cache.by_key.insert(
            request.idempotency_key.clone(),
            (fingerprint, receipt.clone()),
        );
        Ok(receipt)
    }

    /// Recompute a child-process receipt against current app-owned context.
    pub fn verify_receipt(
        &self,
        context: &InvestigationContext,
        request: &InvestigationRequestV1,
        received: &InvestigationReceiptV1,
    ) -> Result<(), String> {
        if received.elapsed_ms > request.budget.max_elapsed_ms {
            return Err("investigation receipt exceeded its elapsed-time budget".into());
        }
        let mut expected = self.investigate(context, request, &AtomicBool::new(false))?;
        expected.elapsed_ms = received.elapsed_ms;
        if expected != *received {
            return Err("investigation receipt does not match verified source evidence".into());
        }
        Ok(())
    }
}

fn execute(
    context: &InvestigationContext,
    request: &InvestigationRequestV1,
    started: std::time::Instant,
    deadline: std::time::Instant,
    cancelled: &AtomicBool,
) -> Result<InvestigationReceiptV1, String> {
    let evidence = evidence_map(&context.profile)?;
    let allowed: BTreeSet<&str> = evidence.keys().map(String::as_str).collect();
    let mut inspected = Vec::new();
    let mut refs = Vec::new();
    let mut findings = Vec::new();
    let mut uncertainty = Vec::new();
    let mut omitted = Vec::new();
    let mut bytes_read = 0u32;
    let mut files_attempted = 0u16;
    let mut outcome = InvestigationOutcomeV1::NotFoundInScope;
    for path in &request.scope.paths {
        if cancelled.load(Ordering::Acquire) {
            outcome = InvestigationOutcomeV1::Cancelled;
            omitted.push(path.clone());
            break;
        }
        if std::time::Instant::now() >= deadline {
            outcome = InvestigationOutcomeV1::InsufficientEvidence;
            omitted.push(path.clone());
            uncertainty.push("The investigation time budget was exhausted.".into());
            break;
        }
        if files_attempted >= request.budget.max_files {
            omitted.push(path.clone());
            continue;
        }
        if !allowed.contains(path.as_str()) {
            omitted.push(path.clone());
            continue;
        }
        let Some(reference) = evidence.get(path) else {
            continue;
        };
        if !request.allowed_evidence_classes.contains(&reference.kind) {
            omitted.push(path.clone());
            continue;
        }
        files_attempted += 1;
        inspected.push(path.clone());
        let remaining_bytes = request.budget.max_bytes.saturating_sub(bytes_read) as usize;
        let (content, digest) = match read_pinned_file_bounded_with_deadline(
            &context.source,
            &context.profile.key,
            path,
            remaining_bytes,
            deadline,
            cancelled,
        ) {
            Ok(file) => file,
            Err(error) if error.contains("revision") => {
                outcome = InvestigationOutcomeV1::StaleSource;
                uncertainty
                    .push("The selected source changed while evidence was being read.".into());
                break;
            }
            Err(error) if error.contains("cancelled") => {
                outcome = InvestigationOutcomeV1::Cancelled;
                omitted.push(path.clone());
                uncertainty.push("The investigation was cancelled while reading evidence.".into());
                break;
            }
            Err(error) if error.contains("wall-time budget") => {
                outcome = InvestigationOutcomeV1::InsufficientEvidence;
                omitted.push(path.clone());
                uncertainty.push(
                    "The investigation time budget was exhausted while reading evidence.".into(),
                );
                break;
            }
            Err(error) => {
                outcome = InvestigationOutcomeV1::InsufficientEvidence;
                omitted.push(path.clone());
                uncertainty.push(format!("Evidence could not be read from {path}: {error}"));
                continue;
            }
        };
        if digest != reference.content_sha256 {
            outcome = InvestigationOutcomeV1::ConflictingEvidence;
            uncertainty.push(format!(
                "Pinned content hash disagrees with the repository profile for {path}."
            ));
            break;
        }
        bytes_read = bytes_read.saturating_add(content.len().min(u32::MAX as usize) as u32);
        refs.push(InvestigationSourceRefV1 {
            evidence_id: format!("investigation:{}:{path}", request.idempotency_key),
            path: path.clone(),
            source_sha: context.profile.key.commit_sha.clone(),
            content_sha256: digest,
        });
        for (line_number, line) in content.lines().enumerate() {
            if line.to_lowercase().contains(&request.query.to_lowercase()) {
                findings.push(format!(
                    "Untrusted repository evidence at {path}:{}: {}",
                    line_number + 1,
                    safe_line(line)
                ));
                outcome = InvestigationOutcomeV1::Findings;
                if findings.len() >= usize::from(request.budget.max_matches) {
                    break;
                }
            }
        }
        if findings.len() >= usize::from(request.budget.max_matches) {
            omitted.extend(request.scope.paths.iter().skip(inspected.len()).cloned());
            break;
        }
    }
    if refs.is_empty() && outcome == InvestigationOutcomeV1::NotFoundInScope {
        outcome = InvestigationOutcomeV1::InsufficientEvidence;
        uncertainty.push("The requested scope contained no verified profile evidence.".into());
    } else if !omitted.is_empty() && outcome == InvestigationOutcomeV1::NotFoundInScope {
        outcome = InvestigationOutcomeV1::InsufficientEvidence;
        uncertainty
            .push("Some requested scope was omitted, so absence cannot be established.".into());
    }
    // Recheck freshness at the acceptance boundary, after reads and hashing.
    if cancelled.load(Ordering::Acquire) || std::time::Instant::now() >= deadline {
        outcome = if cancelled.load(Ordering::Acquire) {
            InvestigationOutcomeV1::Cancelled
        } else {
            InvestigationOutcomeV1::InsufficientEvidence
        };
        findings.clear();
        refs.clear();
        uncertainty
            .push("Investigation ended before source freshness could be revalidated.".into());
    } else if verify_profile_freshness_with_deadline(
        &context.source,
        &context.profile,
        deadline,
        cancelled,
    )
    .is_err()
        || context.source.planning_store.revision().ok().as_deref()
            != Some(request.expected_task_revision.as_str())
    {
        outcome = InvestigationOutcomeV1::StaleSource;
        findings.clear();
        refs.clear();
        uncertainty.push("The repository revision changed before the receipt was accepted.".into());
    }
    let files_read = u16::try_from(refs.len()).unwrap_or(u16::MAX);
    let confidence = match outcome {
        InvestigationOutcomeV1::Findings if omitted.is_empty() => 75,
        InvestigationOutcomeV1::Findings => 55,
        InvestigationOutcomeV1::NotFoundInScope => 60,
        InvestigationOutcomeV1::InsufficientEvidence => 20,
        InvestigationOutcomeV1::ProviderUnavailable
        | InvestigationOutcomeV1::StaleSource
        | InvestigationOutcomeV1::ConflictingEvidence
        | InvestigationOutcomeV1::Cancelled => 0,
    };
    Ok(InvestigationReceiptV1 {
        schema_version: 1,
        idempotency_key: request.idempotency_key.clone(),
        repository_id: request.repository_id.clone(),
        source_sha: request.source_sha.clone(),
        inspected_scope: inspected,
        source_refs: refs,
        findings,
        uncertainty,
        omitted_scope: omitted,
        files_read,
        bytes_read,
        elapsed_ms: u32::try_from(started.elapsed().as_millis()).unwrap_or(u32::MAX),
        confidence,
        outcome,
    })
}
