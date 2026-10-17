use serde::{Deserialize, Serialize};

use super::super::super::protocol::EnvelopeV1;

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub(super) struct JournalEntry {
    pub request_id: String,
    pub request_sha256: String,
    pub created_unix_ms: u64,
    pub state: EntryState,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(tag = "state", rename_all = "snake_case", deny_unknown_fields)]
pub(super) enum EntryState {
    Pending,
    Completed { response: EnvelopeV1 },
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub(super) struct RequestIndexEntry {
    pub request_id: String,
    pub execution_id: String,
    pub request_sha256: String,
    pub created_unix_ms: u64,
    /// Missing state identifies a legacy reservation row. State rows are
    /// append-only and folded by request ID during index compaction.
    #[serde(default)]
    pub state: RequestIndexState,
}

#[derive(Clone, Debug, Default, Deserialize, Serialize, PartialEq, Eq)]
#[serde(tag = "status", rename_all = "snake_case", deny_unknown_fields)]
pub(super) enum RequestIndexState {
    #[default]
    Unknown,
    Reserved,
    Pending,
    Completed {
        completed_unix_ms: u64,
    },
    Expired,
}
