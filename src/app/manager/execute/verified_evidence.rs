use sha2::Digest;

pub(crate) fn verified_broker_evidence(
    authorization: &crate::core::investigation_broker::InvestigationAuthorizationV1,
) -> Result<Vec<(String, String, String)>, String> {
    let metadata = std::fs::symlink_metadata(&authorization.receipt_root)
        .map_err(|_| "Manager investigation receipt handoff disappeared".to_owned())?;
    if metadata.file_type().is_symlink() || !metadata.is_dir() {
        return Err("Manager investigation receipt handoff is invalid".into());
    }
    let mut files = std::fs::read_dir(&authorization.receipt_root)
        .map_err(|_| "Manager investigation receipts could not be read".to_owned())?
        .collect::<Result<Vec<_>, _>>()
        .map_err(|_| "Manager investigation receipts could not be listed".to_owned())?;
    files.sort_by_key(|entry| entry.file_name());
    if files.len() > 8 {
        return Err("Manager investigation exceeded its receipt limit".into());
    }
    let broker = crate::core::investigation_broker::InvestigationBroker::default();
    let mut output = Vec::new();
    for entry in files {
        let file_type = entry
            .file_type()
            .map_err(|_| "Manager receipt metadata is unavailable")?;
        if !file_type.is_file() || file_type.is_symlink() {
            return Err("Manager receipt handoff contains a non-regular file".into());
        }
        let name = entry.file_name().to_string_lossy().into_owned();
        let bytes = std::fs::read(entry.path())
            .map_err(|_| "Manager receipt could not be read".to_owned())?;
        if bytes.len() > 300_000 {
            return Err("Manager receipt exceeded its size limit".into());
        }
        let envelope: crate::core::investigation_broker::InvestigationEvidenceEnvelopeV1 =
            serde_json::from_slice(&bytes)
                .map_err(|_| "Manager receipt is malformed or tampered".to_owned())?;
        if name != format!("{}.json", envelope.request.idempotency_key) {
            return Err("Manager receipt filename does not match its request identity".into());
        }
        let context = authorization.context_for_request(&envelope.request)?;
        broker.verify_receipt(&context, &envelope.request, &envelope.receipt)?;
        let serialized_receipt = serde_json::to_vec(&envelope.receipt)
            .map_err(|_| "verified Manager receipt could not be encoded".to_owned())?;
        let receipt_hash = format!("{:x}", sha2::Sha256::digest(&serialized_receipt));
        let receipt_content = format!(
            "Investigation outcome: {:?}; confidence {}; inspected scope: {}; omitted scope: {}; findings: {}",
            envelope.receipt.outcome,
            envelope.receipt.confidence,
            envelope.receipt.inspected_scope.join(", "),
            envelope.receipt.omitted_scope.join(", "),
            envelope.receipt.findings.join(" | "),
        );
        output.push((
            format!("investigation-receipt:{}", envelope.request.idempotency_key),
            receipt_hash,
            receipt_content,
        ));
        if envelope.request.comparison.is_some() {
            paired_comparison_evidence(&envelope.receipt, &mut output)?;
        } else {
            single_revision_evidence(&envelope.receipt, &mut output);
        }
    }
    Ok(output)
}

fn paired_comparison_evidence(
    receipt: &crate::core::investigation_broker::InvestigationReceiptV1,
    output: &mut Vec<(String, String, String)>,
) -> Result<(), String> {
    for paired in &receipt.comparison_refs {
        let prefix = format!("Untrusted repository evidence at {}:", paired.path);
        let findings = receipt
            .findings
            .iter()
            .filter(|finding| finding.starts_with(&prefix))
            .cloned()
            .collect::<Vec<_>>()
            .join("\n");
        let content = format!(
            "Paired comparison evidence at {}: left {} content {}; right {} content {}; {}",
            paired.path,
            paired.left_source_sha,
            paired.left_content_sha256,
            paired.right_source_sha,
            paired.right_content_sha256,
            findings
        );
        let serialized = serde_json::to_vec(&(paired, &findings))
            .map_err(|_| "verified comparison evidence could not be encoded")?;
        let hash = format!("{:x}", sha2::Sha256::digest(serialized));
        output.push((paired.evidence_id.clone(), hash, content));
    }
    Ok(())
}

fn single_revision_evidence(
    receipt: &crate::core::investigation_broker::InvestigationReceiptV1,
    output: &mut Vec<(String, String, String)>,
) {
    for source in &receipt.source_refs {
        let prefix = format!("Untrusted repository evidence at {}:", source.path);
        let content = receipt
            .findings
            .iter()
            .filter(|finding| finding.starts_with(&prefix))
            .cloned()
            .collect::<Vec<_>>()
            .join("\n");
        if !content.is_empty() {
            output.push((
                source.evidence_id.clone(),
                source.content_sha256.clone(),
                content,
            ));
        }
    }
}
