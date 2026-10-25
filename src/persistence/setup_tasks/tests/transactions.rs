mod atomic;
mod isolation;
mod lifecycle;
mod process;

use std::path::Path;

use crate::app::setup_attention::catalog::FindingStatus;

use super::super::{
    AuthorizationBindingV1, SetupTaskStoreV1, authorization_scope_digest, transaction,
};
use super::support::snapshot;

const TASK_ID: &str = "setup:docker:rootless";
const OTHER_TASK_ID: &str = "setup:images:general:amd64";
const OPERATION_ID: &str = "00000000-0000-4000-8000-000000000101";
const OTHER_OPERATION_ID: &str = "00000000-0000-4000-8000-000000000102";
const RECEIPT_ID: &str = "00000000-0000-4000-8000-000000000103";
const OTHER_RECEIPT_ID: &str = "00000000-0000-4000-8000-000000000104";

fn binding(task_id: &str, operation_id: &str) -> AuthorizationBindingV1 {
    let scopes = vec![task_id.to_owned()];
    AuthorizationBindingV1 {
        receipt_id: if operation_id == OTHER_OPERATION_ID {
            OTHER_RECEIPT_ID.into()
        } else {
            RECEIPT_ID.into()
        },
        scope_digest: authorization_scope_digest(task_id, operation_id, 1, &scopes),
        authorized_at_unix_ms: 12,
    }
}

fn seed_ready(path: &Path, task_ids: &[&str]) {
    transaction::update_at(path, |store| {
        let mut ready = SetupTaskStoreV1 {
            latest_scan_generation: 1,
            ..SetupTaskStoreV1::default()
        };
        let findings = task_ids
            .iter()
            .map(|task_id| (*task_id, FindingStatus::NeedsAttention, &[][..]))
            .collect::<Vec<_>>();
        ready.reconcile(&snapshot(&findings, &[], 10), 1)?;
        *store = ready;
        Ok(((), true))
    })
    .unwrap();
}
