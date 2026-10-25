use crate::app::setup_attention::catalog::FindingStatus;

use super::super::support::{scan, snapshot};
use crate::persistence::setup_tasks::{SetupTaskStateV1, SetupTaskStoreV1};

#[test]
fn positive_git_probe_evidence_verifies_each_matching_git_task() {
    for (id, status, domain) in [
        (
            "setup:git:install",
            FindingStatus::NeedsAttention,
            "git:executable",
        ),
        (
            "setup:git:identity",
            FindingStatus::NeedsAttention,
            "git:identity",
        ),
        ("setup:git:verify", FindingStatus::Unverified, "git"),
    ] {
        let mut store = SetupTaskStoreV1::default();
        scan(&mut store, &snapshot(&[(id, status, &[])], &[], 1)).unwrap();
        scan(&mut store, &snapshot(&[], &[domain], 2)).unwrap();
        assert_eq!(store.tasks[id].state, SetupTaskStateV1::Verified, "{id}");
        assert_eq!(
            store.tasks[id]
                .verified_result
                .as_ref()
                .unwrap()
                .evidence_domain,
            domain,
            "{id}"
        );
    }
}
