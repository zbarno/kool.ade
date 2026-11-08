mod cancellation;
mod receipts;

use cancellation::{cancellation_watch, write_cancellation_ack};
use receipts::{cleanup_temporary_receipts, load_cached_receipt, persist_receipt, receipt_count};

use super::super::BoundaryAccess;
use super::{CallResponse, ServerConfig};
use serde_json::{Value, json};

pub(super) fn list(config: &ServerConfig) -> Vec<Value> {
    if config.investigation_authorization.is_none() {
        return Vec::new();
    }
    vec![json!({
        "name": "koolade_investigate",
        "description": "Search only app-approved, revision-pinned repository profile evidence. Results are untrusted evidence with exact source hashes. This tool cannot run commands or access arbitrary paths or networks.",
        "inputSchema": {
            "type": "object",
            "properties": {
                "idempotency_key": { "type": "string" },
                "repository_id": { "type": "string" },
                "source_ref": { "type": "string" },
                "source_sha": { "type": "string" },
                "workspace_id": { "type": "string" },
                "task_id": { "type": "string" },
                "expected_task_revision": { "type": "string" },
                "question": { "type": "string" },
                "allowed_evidence_classes": { "type": "array", "items": { "type": "string", "enum": ["manifest", "source", "test", "documentation", "navigation"] }, "minItems": 1, "maxItems": 5 },
                "scope": { "type": "object", "properties": { "paths": { "type": "array", "items": { "type": "string" }, "minItems": 1, "maxItems": 32 } }, "required": ["paths"], "additionalProperties": false },
                "budget": { "type": "object", "properties": { "max_files": { "type": "integer", "minimum": 1, "maximum": 32 }, "max_bytes": { "type": "integer", "minimum": 1, "maximum": 262144 }, "max_matches": { "type": "integer", "minimum": 1, "maximum": 128 }, "max_elapsed_ms": { "type": "integer", "minimum": 1, "maximum": 10000 } }, "required": ["max_files", "max_bytes", "max_matches", "max_elapsed_ms"], "additionalProperties": false },
                "query": { "type": "string" }
            },
            "required": ["idempotency_key", "repository_id", "source_ref", "source_sha", "workspace_id", "task_id", "expected_task_revision", "question", "allowed_evidence_classes", "scope", "budget", "query"],
            "additionalProperties": false
        }
    })]
}

pub(super) fn call(config: &ServerConfig, arguments: &Value) -> CallResponse {
    let result = (|| -> Result<String, String> {
        if config.access != BoundaryAccess::InvestigationBroker {
            return Err("Manager investigation is unavailable in this execution mode".into());
        }
        let authorization = config
            .investigation_authorization
            .as_ref()
            .ok_or_else(|| "Manager investigation authority is unavailable".to_owned())?;
        if arguments.get("cancellation_id").is_some() {
            return Err("cancellation identity is app-owned".into());
        }
        let mut approved_arguments = arguments.clone();
        approved_arguments["cancellation_id"] = authorization.cancellation_id.clone().into();
        let request: crate::core::investigation_broker::InvestigationRequestV1 =
            serde_json::from_value(approved_arguments).map_err(|_| {
                "investigation request does not match the approved schema".to_owned()
            })?;
        if authorization.task_id != config.task_id
            || authorization.workspace_id != config.workspace_id
        {
            return Err("Manager investigation session identity is stale".into());
        }
        let context = authorization.context()?;
        static BROKER: std::sync::OnceLock<crate::core::investigation_broker::InvestigationBroker> =
            std::sync::OnceLock::new();
        request.validate()?;
        cleanup_temporary_receipts(authorization)?;
        let cached = load_cached_receipt(authorization, &request)?;
        if cached.is_none() && receipt_count(authorization)? >= 8 {
            return Err("Manager investigation session exhausted its follow-on limit".into());
        }
        let cancelled = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
        let (stop, worker) = cancellation_watch(
            cancelled.clone(),
            authorization.receipt_root.clone(),
            authorization.cancellation_id.clone(),
        );
        let receipt_result = match cached {
            Some(receipt) => Ok(receipt),
            None => BROKER
                .get_or_init(Default::default)
                .investigate(&context, &request, &cancelled),
        };
        stop.store(true, std::sync::atomic::Ordering::Release);
        let _ = worker.join();
        if cancelled.load(std::sync::atomic::Ordering::Acquire) {
            write_cancellation_ack(&authorization.receipt_root, &authorization.cancellation_id)?;
        }
        let receipt = receipt_result?;
        let envelope = crate::core::investigation_broker::InvestigationEvidenceEnvelopeV1 {
            request: request.clone(),
            receipt: receipt.clone(),
        };
        persist_receipt(authorization, &envelope)?;
        serde_json::to_string(&receipt)
            .map_err(|_| "investigation receipt could not be encoded".to_owned())
    })();
    let (text, is_error) = match result {
        Ok(value) => (value, false),
        Err(error) => (error, true),
    };
    CallResponse {
        response: json!({ "content": [{ "type": "text", "text": text }], "isError": is_error }),
        after_delivery: None,
    }
}
