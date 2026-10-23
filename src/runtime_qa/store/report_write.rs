use std::fs;

use super::{RuntimeQaStore, create_or_match_json, safe_uuid};
use crate::runtime_qa::QaRunRequestV1;

impl RuntimeQaStore {
    /// Mark an immutable report write before screenshots or report records
    /// become durable, so restart recovery never repeats completed driver work.
    pub(super) fn start_report_write(&self, request: &QaRunRequestV1) -> Result<(), String> {
        let run = safe_uuid(&request.qa_run_id)?;
        let directory = self.root.join(run);
        fs::create_dir_all(&directory)
            .map_err(|error| format!("cannot create runtime QA record directory: {error}"))?;
        create_or_match_json(&self.root, &directory.join("report-write.json"), request)
    }
}
