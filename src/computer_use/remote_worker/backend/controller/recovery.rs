use super::*;

impl RemoteComputerBackendV1 {
    pub(in crate::computer_use::remote_worker::backend) fn save_recovery_record(
        &self,
        session: &ComputerSessionV1,
        transport_sequence: u64,
        last_event_sequence: u64,
        pending_action_sequence: Option<u64>,
        outcome_unknown: bool,
    ) -> Result<(), Error> {
        let record = ComputerRemoteWorkerRecoveryRecordV1 {
            schema_version: 1,
            worker_id: self.enrollment.worker_id.clone(),
            machine_id: self.enrollment.machine_id.clone(),
            session: session.clone(),
            last_transport_sequence: transport_sequence,
            last_event_sequence,
            pending_action_sequence,
            outcome_unknown,
        };
        record
            .validate(&self.enrollment.worker_id, &self.enrollment.machine_id)
            .map_err(|_| Error::Io)?;
        self.recovery_store.save(&record).map_err(|_| Error::Io)
    }

    pub fn recover_session(
        &self,
        replacement: &ComputerSessionV1,
        last_event_sequence: u64,
        now_unix_ms: u64,
    ) -> Result<(), Error> {
        self.validate_session(replacement, now_unix_ms)?;
        let stored = self
            .recovery_store
            .load(&replacement.identity.session_id)
            .map_err(|_| Error::Io)?
            .ok_or(Error::OwnershipMismatch)?;
        stored
            .validate(&self.enrollment.worker_id, &self.enrollment.machine_id)
            .map_err(|_| Error::Io)?;
        let previous = &stored.session;
        previous
            .verify_attestation(&self.controller_key_id, &self.controller_key)
            .map_err(|_| Error::UntrustedSession)?;
        if replacement.lease.lease_id != previous.lease.lease_id
            || replacement.lease.generation <= previous.lease.generation
            || replacement.cancellation_generation <= previous.cancellation_generation
            || replacement.approval_generation <= previous.approval_generation
            || replacement.identity != previous.identity
            || replacement.target != previous.target
            || replacement.image_digest != previous.image_digest
            || replacement.image_manifest_digest != previous.image_manifest_digest
            || replacement.image_signer_key_id != previous.image_signer_key_id
            || last_event_sequence < stored.last_event_sequence
        {
            return Err(Error::StaleLease);
        }
        // Persist the recovery attempt before connecting. If acknowledgement
        // is lost, the outcome remains uncertain and the attempt is not replayed.
        self.save_recovery_record(
            replacement,
            0,
            last_event_sequence,
            stored.pending_action_sequence,
            true,
        )?;
        let mut client = ComputerRemoteWorkerClientV1::connect_for_recovery(
            &self.ssh,
            &self.enrollment.worker_id,
            &self.enrollment.ssh_host_key_fingerprint,
            std::path::Path::new(&self.enrollment.worker_config_path),
        )
        .map_err(|_| Error::HostNotReady)?;
        let mut request = self.request_for(
            replacement,
            Body::Recover {
                replacement_session: replacement.clone(),
                last_event_sequence,
            },
            client.next_transport_sequence(),
        );
        request.transport_sequence = 0;
        let response = client.request(&request).map_err(|_| Error::Io)?;
        if !matches!(response.result, ResultValue::Acknowledged) {
            return Err(Error::Io);
        }
        let handle = ComputerMachineHandleV1::from_remote(
            replacement.identity.session_id.clone(),
            replacement.lease.lease_id.clone(),
            replacement.lease.generation,
            self.enrollment.machine_id.clone(),
        );
        self.save_recovery_record(
            replacement,
            1,
            last_event_sequence,
            stored.pending_action_sequence,
            stored.pending_action_sequence.is_some(),
        )?;
        self.sessions.lock().map_err(|_| Error::Io)?.insert(
            replacement.identity.session_id.clone(),
            ActiveRemoteSession {
                session: replacement.clone(),
                handle,
                client,
                last_event_sequence,
            },
        );
        Ok(())
    }
}
