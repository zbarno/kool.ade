use super::{RuntimeQaStore, create_or_match_json, read_json, safe_uuid};
use crate::runtime_qa::{QaPreviewRelayBindingV1, QaRunRequestV1, QaTaskServiceLeaseV1};

impl RuntimeQaStore {
    /// Persist an exact service lease before opening the QA network path.
    pub fn save_service_lease(
        &self,
        request: &QaRunRequestV1,
        lease: &QaTaskServiceLeaseV1,
    ) -> Result<(), String> {
        lease.validate_for(request)?;
        let run = safe_uuid(&request.qa_run_id)?;
        create_or_match_json(&self.root.join(run).join("service-lease.json"), lease)
    }

    pub fn load_service_lease(
        &self,
        request: &QaRunRequestV1,
    ) -> Result<QaTaskServiceLeaseV1, String> {
        let run = safe_uuid(&request.qa_run_id)?;
        let lease: QaTaskServiceLeaseV1 =
            read_json(&self.root.join(run).join("service-lease.json"))?;
        lease.validate_for(request)?;
        Ok(lease)
    }

    /// Bind the real inspected, owned driver ID durably before container start.
    pub fn save_relay_binding(
        &self,
        request: &QaRunRequestV1,
        scenario_uid: &str,
        binding: &QaPreviewRelayBindingV1,
    ) -> Result<(), String> {
        request.validate()?;
        binding.validate()?;
        if binding.qa_run_id != request.qa_run_id
            || binding.task_uid != request.parent_task_uid
            || binding.task_execution_id != request.execution_session_id
            || binding.task_build_sha != request.build_artifact_sha256
            || binding.approved_spec_hash != request.approved_spec_hash
            || binding.startup_recipe_id != request.startup_recipe_ref
            || binding.startup_recipe_sha256 != request.startup_recipe_sha256
        {
            return Err("QA relay binding does not match the immutable run".into());
        }
        safe_uuid(scenario_uid)?;
        let run = safe_uuid(&request.qa_run_id)?;
        create_or_match_json(
            &self
                .root
                .join(run)
                .join(format!("relay-binding-{scenario_uid}.json")),
            binding,
        )
    }

    pub fn load_relay_binding(
        &self,
        request: &QaRunRequestV1,
        scenario_uid: &str,
    ) -> Result<QaPreviewRelayBindingV1, String> {
        let run = safe_uuid(&request.qa_run_id)?;
        let binding: QaPreviewRelayBindingV1 = read_json(
            &self
                .root
                .join(run)
                .join(format!("relay-binding-{scenario_uid}.json")),
        )?;
        self.save_relay_binding(request, scenario_uid, &binding)?;
        Ok(binding)
    }
}
