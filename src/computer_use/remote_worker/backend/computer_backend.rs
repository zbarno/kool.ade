use super::*;

impl ComputerBackend for RemoteComputerBackendV1 {
    fn probe(&self) -> Result<ComputerBackendProbeV1, Error> {
        let advertisement = self.fetch_advertisement()?;
        Ok(ComputerBackendProbeV1 {
            linux_host: true,
            kvm_accessible: advertisement.cpu_virtualization_available,
            qemu_available: true,
            delegated_resource_controls: true,
            max_vcpus: (advertisement.cpu_millis / 1_000).min(u32::from(u16::MAX)) as u16,
            max_memory_bytes: advertisement
                .memory_bytes
                .min(self.enrollment.max_memory_bytes),
            max_overlay_bytes: advertisement.disk_bytes.min(self.enrollment.max_disk_bytes),
            max_processes: 32,
            parallel_slots_per_machine: advertisement
                .parallel_slots
                .min(self.enrollment.max_parallel_sessions),
            boot_timeout: Duration::from_secs(120),
        })
    }

    fn prepare(
        &self,
        session: &ComputerSessionV1,
        overlay: GuestImageOverlayV1,
        now: u64,
    ) -> Result<(ComputerMachineHandleV1, ComputerBackendEventV1), Error> {
        self.validate_session(session, now)?;
        let advertisement = self.fetch_advertisement()?;
        self.validate_advertisement_for_session(session, &advertisement, now)?;
        overlay
            .release_after_vm_stopped()
            .map_err(|_| Error::InvalidOverlay)?;
        let mut client = ComputerRemoteWorkerClientV1::connect(
            &self.ssh,
            &self.enrollment.worker_id,
            &self.enrollment.ssh_host_key_fingerprint,
            std::path::Path::new(&self.enrollment.worker_config_path),
        )
        .map_err(|_| Error::HostNotReady)?;
        let request = self.request_for(
            session,
            Body::Start {
                session: session.clone(),
            },
            client.next_transport_sequence(),
        );
        let transport_sequence = request.transport_sequence;
        self.save_recovery_record(session, transport_sequence, 0, None, true)?;
        let response = client.request(&request).map_err(|_| Error::Io)?;
        let ResultValue::Prepared { handle, event } = response.result else {
            return Err(Error::Io);
        };
        if handle.machine_id() != self.enrollment.machine_id {
            return Err(Error::OwnershipMismatch);
        }
        self.sessions.lock().map_err(|_| Error::Io)?.insert(
            session.identity.session_id.clone(),
            ActiveRemoteSession {
                session: session.clone(),
                handle: handle.clone(),
                client,
                last_event_sequence: event.sequence,
            },
        );
        self.save_recovery_record(session, transport_sequence, event.sequence, None, false)?;
        Ok((handle, event))
    }

    fn recover(&self) -> Result<Vec<ComputerBackendEventV1>, Error> {
        let records = self.recovery_store.list().map_err(|_| Error::Io)?;
        if records.is_empty() {
            Ok(Vec::new())
        } else {
            // Startup may expose this durable intent for explicit operator
            // recovery, but it never silently resumes a prior authorization.
            Err(Error::HostNotReady)
        }
    }

    fn boot(
        &self,
        s: &ComputerSessionV1,
        h: &ComputerMachineHandleV1,
        _: u64,
    ) -> Result<ComputerBackendEventV1, Error> {
        Self::map_event(self.send_operation(s, Operation::Boot { handle: h.clone() })?)
    }
    fn wait_ready(
        &self,
        s: &ComputerSessionV1,
        h: &ComputerMachineHandleV1,
        timeout: Duration,
        _: u64,
    ) -> Result<ComputerBackendEventV1, Error> {
        Self::map_event(self.send_operation(
            s,
            Operation::WaitReady {
                handle: h.clone(),
                timeout_ms: timeout.as_millis().min(u32::MAX as u128) as u32,
            },
        )?)
    }
    fn connect(
        &self,
        _: &ComputerSessionV1,
        _: &ComputerMachineHandleV1,
        _: u64,
    ) -> Result<ComputerGuestChannelV1, Error> {
        Err(Error::GuestChannelUnavailable)
    }
    fn authenticate_guest(
        &self,
        session: &ComputerSessionV1,
        handle: &ComputerMachineHandleV1,
        _: u64,
    ) -> Result<AuthenticatedGuestChannelV1, Error> {
        let result = self.send_operation(
            session,
            Operation::AuthenticateGuest {
                handle: handle.clone(),
            },
        )?;
        let ResultValue::GuestAuthenticated {
            readiness,
            capabilities,
            transcript_sha256,
        } = result
        else {
            return Err(Error::GuestReadinessUnverified);
        };
        let context = GuestAuthenticatedContextV1 {
            session_id: session.identity.session_id.clone(),
            worker_id: self.enrollment.worker_id.clone(),
            lease_generation: session.lease.generation,
            expires_at_unix_ms: session.lease.expires_at_unix_ms,
            boot_id: readiness.guest_boot_id,
        };
        Ok(AuthenticatedGuestChannelV1::from_remote(
            context,
            capabilities,
            transcript_sha256,
            Arc::new(RemoteGuestActionTransport {
                enrollment: self.enrollment.clone(),
                sessions: self.sessions.clone(),
                recovery_store: self.recovery_store.clone(),
            }),
        ))
    }
    fn open_guest_action_journal(
        &self,
        session: &ComputerSessionV1,
        handle: &ComputerMachineHandleV1,
        _: u64,
    ) -> Result<GuestActionJournalV1, Error> {
        let sessions = self.sessions.lock().map_err(|_| Error::Io)?;
        let active = sessions
            .get(&session.identity.session_id)
            .ok_or(Error::OwnershipMismatch)?;
        if active.handle != *handle {
            return Err(Error::StaleLease);
        }
        let path = self.journal_root.join(&session.identity.session_id);
        ensure_private_dir(&path)?;
        GuestActionJournalV1::open(
            &path,
            session.identity.session_id.clone(),
            session.lease.generation,
            session.action_sequence,
            session.policy.resource_budget.action_limit,
        )
        .map_err(|_| Error::Io)
    }
    fn capture_display_frame(
        &self,
        s: &ComputerSessionV1,
        h: &ComputerMachineHandleV1,
        _: u64,
    ) -> Result<ComputerDisplayFrameV1, Error> {
        match self.send_operation(s, Operation::CaptureDisplay { handle: h.clone() })? {
            ResultValue::DisplayFrame(frame) => Ok(frame),
            _ => Err(Error::Io),
        }
    }
    fn send_display_input(
        &self,
        s: &ComputerSessionV1,
        h: &ComputerMachineHandleV1,
        input: &ComputerDisplayInputV1,
        _: u64,
    ) -> Result<(), Error> {
        match self.send_operation(
            s,
            Operation::SendDisplayInput {
                handle: h.clone(),
                input: input.clone(),
            },
        )? {
            ResultValue::Acknowledged => Ok(()),
            _ => Err(Error::Io),
        }
    }
    fn mark_guest_ready(
        &self,
        s: &ComputerSessionV1,
        h: &ComputerMachineHandleV1,
        _: ComputerGuestReadinessV1,
        _: u64,
    ) -> Result<ComputerBackendEventV1, Error> {
        Self::map_event(self.send_operation(
            s,
            Operation::WaitReady {
                handle: h.clone(),
                timeout_ms: 1,
            },
        )?)
    }
    fn suspend(
        &self,
        s: &ComputerSessionV1,
        h: &ComputerMachineHandleV1,
        _: u64,
    ) -> Result<ComputerBackendEventV1, Error> {
        Self::map_event(self.send_operation(s, Operation::Suspend { handle: h.clone() })?)
    }
    fn resume(
        &self,
        s: &ComputerSessionV1,
        h: &ComputerMachineHandleV1,
        _: u64,
    ) -> Result<ComputerBackendEventV1, Error> {
        Self::map_event(self.send_operation(s, Operation::Resume { handle: h.clone() })?)
    }
    fn snapshot(
        &self,
        s: &ComputerSessionV1,
        h: &ComputerMachineHandleV1,
        _: u64,
    ) -> Result<ComputerMachineRecordV1, Error> {
        Self::map_record(self.send_operation(s, Operation::Snapshot { handle: h.clone() })?)
    }
    fn cancel(
        &self,
        session: &ComputerSessionV1,
        handle: &ComputerMachineHandleV1,
        _: u64,
    ) -> Result<ComputerBackendEventV1, Error> {
        self.validate_session(session, now_ms()?)?;
        let mut sessions = self.sessions.lock().map_err(|_| Error::Io)?;
        let active = sessions
            .get_mut(&session.identity.session_id)
            .ok_or(Error::OwnershipMismatch)?;
        if active.handle != *handle
            || session.cancellation_generation <= active.session.cancellation_generation
        {
            return Err(Error::StaleLease);
        }
        let req = self.request_for(
            &active.session,
            Body::Cancel {
                replacement_session: session.clone(),
            },
            active.client.next_transport_sequence(),
        );
        let response = active.client.request(&req).map_err(|_| Error::Io)?;
        active.session = session.clone();
        match response.result {
            ResultValue::BackendEvent(event) => Ok(event),
            _ => Err(Error::Io),
        }
    }
    fn stop(
        &self,
        s: &ComputerSessionV1,
        h: &ComputerMachineHandleV1,
        _: u64,
    ) -> Result<ComputerBackendEventV1, Error> {
        Self::map_event(self.send_operation(s, Operation::Stop { handle: h.clone() })?)
    }
    fn collect(
        &self,
        s: &ComputerSessionV1,
        h: &ComputerMachineHandleV1,
        _: u64,
    ) -> Result<ComputerMachineRecordV1, Error> {
        Self::map_record(self.send_operation(s, Operation::Collect { handle: h.clone() })?)
    }
    fn release(
        &self,
        s: &ComputerSessionV1,
        h: &ComputerMachineHandleV1,
        _: u64,
    ) -> Result<ComputerBackendEventV1, Error> {
        let event =
            Self::map_event(self.send_operation(s, Operation::Release { handle: h.clone() })?)?;
        self.sessions
            .lock()
            .map_err(|_| Error::Io)?
            .remove(&s.identity.session_id);
        Ok(event)
    }
}
