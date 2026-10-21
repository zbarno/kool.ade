use super::*;

impl ExecutionSession {
    /// Request a worker-authenticated dependency relay for the exact selected
    /// task workspace. Local broker authorization remains authoritative.
    pub fn relay_dependency_request(
        &self,
        need: crate::harness::DependencyNeed,
    ) -> Result<crate::execution::remote::dependency::DependencyRelayRequestV1, ExecutionError>
    {
        if let Some(destination) = self.retry_destination()? {
            return destination.relay_dependency_request(need);
        }
        self.verify_authorization()?;
        let cancellation_generation = self.cancellation_generation.load(Ordering::Acquire);
        if cancellation_generation == 0 || self.cancel_requested.load(Ordering::Acquire) {
            return Err(policy_error("Dependency relay was cancelled"));
        }
        let worker_id = match &self.target.location {
            crate::execution::contract::TargetLocation::Remote { worker_id } => worker_id.clone(),
            crate::execution::contract::TargetLocation::Local => {
                return Err(policy_error("Dependency relay requires a remote worker"));
            }
        };
        let request = crate::execution::remote::dependency::DependencyRelayRequestV1 {
            request_id: uuid::Uuid::new_v4().hyphenated().to_string(),
            worker_id,
            execution_id: uuid::Uuid::new_v4().hyphenated().to_string(),
            task_id: self.workspace.task_id.clone(),
            workspace: self.workspace.clone(),
            authorization_generation: self.authorization_generation.clone(),
            cancellation_generation,
            need,
        };
        request
            .validate()
            .map_err(|_| policy_error("Dependency relay request is invalid"))?;
        let emitted = self
            .runtime
            .block_on(self.backend.relay_dependency_request(request.clone()))?;
        self.verify_authorization()?;
        if self.cancel_requested.load(Ordering::Acquire)
            || self.cancellation_generation.load(Ordering::Acquire) != cancellation_generation
        {
            return Err(policy_error("Dependency relay was cancelled"));
        }
        if request != emitted {
            return Err(policy_error(
                "Remote dependency request identity changed during relay",
            ));
        }
        self.remote_dependency_requests
            .lock()
            .map_err(|_| internal_error())?
            .insert(request.request_id.clone(), emitted.clone());
        Ok(emitted)
    }

    /// Return a controller broker decision to the worker that emitted the
    /// matching request. A stale authorization generation is rejected.
    pub fn relay_dependency_decision(
        &self,
        response: crate::execution::remote::dependency::DependencyRelayResponseV1,
    ) -> Result<(), ExecutionError> {
        if let Some(destination) = self.retry_destination()? {
            return destination.relay_dependency_decision(response);
        }
        self.verify_authorization()?;
        if self.cancel_requested.load(Ordering::Acquire)
            || response.cancellation_generation
                != self.cancellation_generation.load(Ordering::Acquire)
        {
            return Err(policy_error(
                "Dependency decision belongs to a cancelled execution",
            ));
        }
        let request = self
            .remote_dependency_requests
            .lock()
            .map_err(|_| internal_error())?
            .get(&response.request_id)
            .cloned()
            .ok_or_else(|| policy_error("Dependency decision has no pending worker request"))?;
        if !request.matches_response(&response)
            || response.authorization_generation != self.authorization_generation
        {
            return Err(policy_error(
                "Dependency decision is outside the active execution authorization",
            ));
        }
        self.runtime
            .block_on(self.backend.relay_dependency_decision(response))?;
        self.verify_authorization()?;
        self.remote_dependency_requests
            .lock()
            .map_err(|_| internal_error())?
            .remove(&request.request_id);
        Ok(())
    }
}
