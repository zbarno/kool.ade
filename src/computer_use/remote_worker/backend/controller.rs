use super::*;

impl RemoteComputerBackendV1 {
    /// Return durable recovery intent for explicit app-mediated authorization.
    /// Records never contain keys or artifact payloads.
    pub fn recovery_records(&self) -> Result<Vec<ComputerRemoteWorkerRecoveryRecordV1>, Error> {
        self.recovery_store.list().map_err(|_| Error::Io)
    }

    /// Copy bytes already authorized and staged by ComputerArtifactBridge into
    /// the guest Downloads directory. Only a safe display name and bounded
    /// chunks cross the worker wire; no host path is accepted.
    /// Read one named guest Downloads file in bounded chunks. The caller must
    /// pass the complete result to ComputerArtifactBridge for grant/revision,
    /// signature, and quota validation before publishing it.
    pub(super) fn remote_handle(
        &self,
        session: &ComputerSessionV1,
    ) -> Result<ComputerMachineHandleV1, Error> {
        let sessions = self.sessions.lock().map_err(|_| Error::Io)?;
        let active = sessions
            .get(&session.identity.session_id)
            .ok_or(Error::OwnershipMismatch)?;
        if active.session.lease.generation != session.lease.generation
            || active.session.target != session.target
        {
            return Err(Error::StaleLease);
        }
        Ok(active.handle.clone())
    }

    pub fn new(
        enrollment: ComputerRemoteWorkerEnrollmentV1,
        _known_hosts_path: PathBuf,
        _ssh_binary: PathBuf,
        controller_key_id: String,
        controller_key: VerifyingKey,
        journal_root: PathBuf,
    ) -> Result<Self, Error> {
        enrollment
            .validate()
            .map_err(|_| Error::UnsupportedTarget)?;
        let ssh = enrollment
            .ssh_config()
            .map_err(|_| Error::UnsupportedTarget)?;
        if !canonical_uuid(&controller_key_id) {
            return Err(Error::UntrustedSession);
        }
        ensure_private_dir(&journal_root)?;
        let recovery_store =
            ComputerRemoteWorkerSessionStoreV1::open(journal_root.join("session-recovery"))
                .map_err(|_| Error::UnsafeRuntimeDirectory)?;
        Ok(Self {
            enrollment,
            ssh,
            controller_key_id,
            controller_key,
            journal_root,
            sessions: Arc::new(Mutex::new(HashMap::new())),
            recovery_store,
        })
    }

    pub(super) fn validate_session(
        &self,
        session: &ComputerSessionV1,
        now: u64,
    ) -> Result<(), Error> {
        session
            .validate_for_host(now, &self.controller_key_id, &self.controller_key)
            .map_err(|_| Error::UntrustedSession)?;
        let crate::computer_use::ComputerTargetV1::RemoteVmWorker {
            worker_id,
            machine_id,
            trust,
        } = &session.target
        else {
            return Err(Error::UnsupportedTarget);
        };
        if !self.enrollment.enabled
            || worker_id != &self.enrollment.worker_id
            || machine_id != &self.enrollment.machine_id
            || trust.ssh_host_key_fingerprint != self.enrollment.ssh_host_key_fingerprint
            || !self
                .enrollment
                .trusted_image_digests
                .contains(&session.image_digest)
            || session.policy.resource_budget.cpu_millis > self.enrollment.max_vcpus as u32 * 1_000
            || session.policy.resource_budget.memory_bytes > self.enrollment.max_memory_bytes
            || session.policy.resource_budget.disk_bytes > self.enrollment.max_disk_bytes
        {
            return Err(Error::UnsupportedPolicy);
        }
        Ok(())
    }

    pub(super) fn fetch_advertisement(&self) -> Result<ComputerRemoteWorkerAdvertisementV1, Error> {
        let mut client = ComputerRemoteWorkerClientV1::connect(
            &self.ssh,
            &self.enrollment.worker_id,
            &self.enrollment.ssh_host_key_fingerprint,
            std::path::Path::new(&self.enrollment.worker_config_path),
        )
        .map_err(|_| Error::HostNotReady)?;
        let session_id = uuid::Uuid::new_v4().to_string();
        let request = Request {
            protocol_version: self.enrollment.transport_protocol_version,
            worker_id: self.enrollment.worker_id.clone(),
            ssh_host_key_fingerprint: self.enrollment.ssh_host_key_fingerprint.clone(),
            session_id: session_id.clone(),
            lease_id: uuid::Uuid::new_v4().to_string(),
            lease_generation: 1,
            approval_generation: 1,
            cancellation_generation: 1,
            transport_sequence: client.next_transport_sequence(),
            request_id: uuid::Uuid::new_v4().to_string(),
            body: Body::Probe,
        };
        let response = client.request(&request).map_err(|_| Error::Io)?;
        if response.session_id != session_id {
            return Err(Error::StaleLease);
        }
        let ResultValue::Advertisement(advertisement) = response.result else {
            return Err(Error::HostNotReady);
        };
        client.close().map_err(|_| Error::Io)?;
        let digest = self
            .enrollment
            .trusted_image_digests
            .iter()
            .next()
            .ok_or(Error::UnsupportedPolicy)?;
        let requirements = ComputerRemoteWorkerRequirementsV1 {
            image_digest: digest.clone(),
            cpu_millis: 100,
            memory_bytes: 256 * 1024 * 1024,
            disk_bytes: 512 * 1024 * 1024,
            require_display_streaming: true,
            session_class: crate::computer_use::ComputerSessionClassV1::Disposable,
        };
        if assess_remote_computer_worker_v1(
            Some(&self.enrollment),
            &advertisement,
            &requirements,
            now_ms()?,
        ) != RemoteComputerWorkerEligibilityV1::Eligible
        {
            return Err(Error::HostNotReady);
        }
        Ok(advertisement)
    }

    pub(super) fn validate_advertisement_for_session(
        &self,
        session: &ComputerSessionV1,
        advertisement: &ComputerRemoteWorkerAdvertisementV1,
        now_unix_ms: u64,
    ) -> Result<(), Error> {
        let requirements = ComputerRemoteWorkerRequirementsV1 {
            image_digest: session.image_digest.clone(),
            cpu_millis: session.policy.resource_budget.cpu_millis,
            memory_bytes: session.policy.resource_budget.memory_bytes,
            disk_bytes: session.policy.resource_budget.disk_bytes,
            require_display_streaming: true,
            session_class: session.session_class,
        };
        if assess_remote_computer_worker_v1(
            Some(&self.enrollment),
            advertisement,
            &requirements,
            now_unix_ms,
        ) != RemoteComputerWorkerEligibilityV1::Eligible
        {
            return Err(Error::UnsupportedPolicy);
        }
        Ok(())
    }

    pub(super) fn send_operation(
        &self,
        session: &ComputerSessionV1,
        operation: Operation,
    ) -> Result<ResultValue, Error> {
        self.validate_session(session, now_ms()?)?;
        let prior = self
            .recovery_store
            .load(&session.identity.session_id)
            .map_err(|_| Error::Io)?
            .ok_or(Error::OwnershipMismatch)?;
        if prior.pending_action_sequence.is_some()
            && matches!(&operation, Operation::GuestAction { .. })
        {
            return Err(Error::StaleLease);
        }
        let mut sessions = self.sessions.lock().map_err(|_| Error::Io)?;
        let active = sessions
            .get_mut(&session.identity.session_id)
            .ok_or(Error::OwnershipMismatch)?;
        if active.session.lease.lease_id != session.lease.lease_id
            || active.session.lease.generation != session.lease.generation
            || active.session.cancellation_generation != session.cancellation_generation
        {
            return Err(Error::StaleLease);
        }
        let releases_session = matches!(&operation, Operation::Release { .. });
        let request = self.request_for(
            &active.session,
            Body::Operation { operation },
            active.client.next_transport_sequence(),
        );
        let sequence = request.transport_sequence;
        let pending_action = match &request.body {
            Body::Operation {
                operation: Operation::GuestAction { action, .. },
            } => Some(action.action_sequence),
            _ => None,
        };
        self.save_recovery_record(
            &active.session,
            sequence,
            active.last_event_sequence,
            pending_action,
            true,
        )?;
        let response = active.client.request(&request).map_err(|_| Error::Io)?;
        let event_sequence = match &response.result {
            ResultValue::BackendEvent(event) => event.sequence,
            ResultValue::Prepared { event, .. } => event.sequence,
            ResultValue::MachineRecord(_) => active.last_event_sequence,
            _ => active.last_event_sequence,
        };
        active.last_event_sequence = active.last_event_sequence.max(event_sequence);
        self.save_recovery_record(
            &active.session,
            sequence,
            active.last_event_sequence,
            None,
            false,
        )?;
        if releases_session {
            self.recovery_store
                .remove(&session.identity.session_id)
                .map_err(|_| Error::Io)?;
        }
        Ok(response.result)
    }

    pub(super) fn request_for(
        &self,
        session: &ComputerSessionV1,
        body: Body,
        sequence: u64,
    ) -> Request {
        Request {
            protocol_version: self.enrollment.transport_protocol_version,
            worker_id: self.enrollment.worker_id.clone(),
            ssh_host_key_fingerprint: self.enrollment.ssh_host_key_fingerprint.clone(),
            session_id: session.identity.session_id.clone(),
            lease_id: session.lease.lease_id.clone(),
            lease_generation: session.lease.generation,
            approval_generation: session.approval_generation,
            cancellation_generation: session.cancellation_generation,
            transport_sequence: sequence,
            request_id: uuid::Uuid::new_v4().to_string(),
            body,
        }
    }

    pub(super) fn map_event(result: ResultValue) -> Result<ComputerBackendEventV1, Error> {
        match result {
            ResultValue::BackendEvent(event) => Ok(event),
            _ => Err(Error::Io),
        }
    }
    pub(super) fn map_record(result: ResultValue) -> Result<ComputerMachineRecordV1, Error> {
        match result {
            ResultValue::MachineRecord(record) => Ok(record),
            _ => Err(Error::Io),
        }
    }
}

mod artifacts;
mod recovery;
