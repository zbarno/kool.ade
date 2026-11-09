use super::*;
use crate::computer_use::mcp::{ComputerUseConsentBrokerV1, ComputerUseObservationKindV1};

#[test]
fn cancelled_request_never_reaches_controller_operations() {
    let operations = Arc::new(Operations(AtomicUsize::new(0)));
    let host = RegistryBoundComputerUseMcpHostV1::new(
        Arc::new(ComputerUseAuthorityRegistryV1::default()),
        ComputerUseConsentBrokerV1::default(),
        operations.clone(),
    );
    let cancelled = AtomicBool::new(true);

    let result = host.handle(
        &caller(),
        Request::RequestSession {
            purpose: "inspect synthetic application".into(),
        },
        &cancelled,
    );

    assert_eq!(result.status, Status::Denied);
    assert_eq!(result.reason_code.as_deref(), Some("cancelled"));
    assert_eq!(operations.0.load(Ordering::Relaxed), 0);
}

#[test]
fn session_id_mismatch_is_denied_before_controller_dispatch() {
    let operations = Arc::new(Operations(AtomicUsize::new(0)));
    let host = RegistryBoundComputerUseMcpHostV1::new(
        Arc::new(ComputerUseAuthorityRegistryV1::default()),
        ComputerUseConsentBrokerV1::default(),
        operations.clone(),
    );

    let result = host.handle(
        &caller(),
        Request::Observe {
            session_id: "00000000-0000-4000-8000-000000000009".into(),
            observation: ComputerUseObservationKindV1::Screenshot,
        },
        &AtomicBool::new(false),
    );

    assert_eq!(result.status, Status::Denied);
    assert_eq!(result.reason_code.as_deref(), Some("authorization_failed"));
    assert_eq!(operations.0.load(Ordering::Relaxed), 0);
}

#[test]
fn request_session_waits_for_explicit_app_consent_before_controller_work() {
    let operations = Arc::new(Operations(AtomicUsize::new(0)));
    let consent = ComputerUseConsentBrokerV1::default();
    let host = Arc::new(RegistryBoundComputerUseMcpHostV1::new(
        Arc::new(ComputerUseAuthorityRegistryV1::default()),
        consent.clone(),
        operations.clone(),
    ));
    let thread_host = host.clone();
    let thread_caller = caller();
    let cancelled = Arc::new(AtomicBool::new(false));
    let thread_cancelled = cancelled.clone();
    let requester = std::thread::spawn(move || {
        thread_host.handle(
            &thread_caller,
            Request::RequestSession {
                purpose: "inspect a synthetic app".into(),
            },
            &thread_cancelled,
        )
    });

    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(2);
    let pending = loop {
        if let Some(request) = consent.pending().into_iter().next() {
            break request;
        }
        assert!(
            std::time::Instant::now() < deadline,
            "consent request was not enqueued"
        );
        std::thread::sleep(std::time::Duration::from_millis(1));
    };
    assert_eq!(operations.0.load(Ordering::Relaxed), 0);
    consent.decide(&pending.request_id, true).unwrap();
    assert_eq!(requester.join().unwrap().status, Status::Blocked);
    assert_eq!(operations.0.load(Ordering::Relaxed), 1);
}

fn approve_pending(consent: &ComputerUseConsentBrokerV1) {
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(2);
    let pending = loop {
        if let Some(pending) = consent.pending().into_iter().next() {
            break pending;
        }
        assert!(
            std::time::Instant::now() < deadline,
            "consent request was not enqueued"
        );
        std::thread::sleep(std::time::Duration::from_millis(1));
    };
    consent.decide(&pending.request_id, true).unwrap();
}

fn start_request_and_approve(
    host: Arc<RegistryBoundComputerUseMcpHostV1>,
    consent: ComputerUseConsentBrokerV1,
    caller: ComputerUseCallerV1,
    cancelled: Arc<AtomicBool>,
) -> Response {
    let thread_cancelled = cancelled.clone();
    let requester = std::thread::spawn(move || {
        host.handle(
            &caller,
            Request::RequestSession {
                purpose: "inspect a synthetic app".into(),
            },
            &thread_cancelled,
        )
    });
    approve_pending(&consent);
    requester.join().unwrap()
}

#[test]
fn authority_registered_before_a_blocked_result_is_revoked() {
    let caller = caller();
    let registry = Arc::new(ComputerUseAuthorityRegistryV1::default());
    let consent = ComputerUseConsentBrokerV1::default();
    let operations = Arc::new(RegisterThenFinish {
        registry: registry.clone(),
        status: Status::Blocked,
        cancel_during_startup: false,
    });
    let host = Arc::new(RegistryBoundComputerUseMcpHostV1::new(
        registry.clone(),
        consent.clone(),
        operations,
    ));

    let result = start_request_and_approve(
        host,
        consent,
        caller.clone(),
        Arc::new(AtomicBool::new(false)),
    );

    assert_eq!(result.status, Status::Blocked);
    assert!(matches!(
        registry.authorize(
            &caller.identity,
            caller.actor,
            &caller.role_id,
            unix_time_ms()
        ),
        Err(crate::computer_use::guest_protocol::GuestProtocolErrorV1::StaleLease)
    ));
}

#[test]
fn cancellation_during_startup_revokes_registered_authority_before_ready() {
    let caller = caller();
    let registry = Arc::new(ComputerUseAuthorityRegistryV1::default());
    let consent = ComputerUseConsentBrokerV1::default();
    let operations = Arc::new(RegisterThenFinish {
        registry: registry.clone(),
        status: Status::Ready,
        cancel_during_startup: true,
    });
    let host = Arc::new(RegistryBoundComputerUseMcpHostV1::new(
        registry.clone(),
        consent.clone(),
        operations,
    ));

    let result = start_request_and_approve(
        host,
        consent,
        caller.clone(),
        Arc::new(AtomicBool::new(false)),
    );

    assert_eq!(result.status, Status::Denied);
    assert_eq!(result.reason_code.as_deref(), Some("cancelled"));
    assert!(matches!(
        registry.authorize(
            &caller.identity,
            caller.actor,
            &caller.role_id,
            unix_time_ms()
        ),
        Err(crate::computer_use::guest_protocol::GuestProtocolErrorV1::StaleLease)
    ));
}

struct CancelDuringDispatch {
    cleanups: AtomicUsize,
}

impl ComputerUseMcpOperationsV1 for CancelDuringDispatch {
    fn capabilities(&self, _caller: &ComputerUseCallerV1) -> Response {
        response(Status::Ready)
    }

    fn consent_scope(
        &self,
        _caller: &ComputerUseCallerV1,
    ) -> Result<super::super::super::ComputerUseConsentScopeV1, Box<Response>> {
        unreachable!("this test exercises an already-authorized session")
    }

    fn request_session(
        &self,
        _caller: &ComputerUseCallerV1,
        _grant: &super::super::super::ComputerUseConsentGrantV1,
        _cancelled: &AtomicBool,
    ) -> Response {
        unreachable!("this test exercises an already-authorized session")
    }

    fn dispatch_authorized(
        &self,
        _caller: &ComputerUseCallerV1,
        _authority: Arc<AuthorizedComputerUseSessionV1>,
        _request: Request,
        cancelled: &AtomicBool,
    ) -> Response {
        cancelled.store(true, Ordering::Release);
        response(Status::Completed)
    }

    fn revoke_session(&self, caller: &ComputerUseCallerV1, session_id: &str) -> Response {
        assert_eq!(caller.identity.session_id, session_id);
        self.cleanups.fetch_add(1, Ordering::Relaxed);
        Response {
            session_id: Some(session_id.to_owned()),
            ..response(Status::Completed)
        }
    }
}

#[test]
fn cancellation_during_authorized_dispatch_runs_session_cleanup() {
    let caller = caller();
    let registry = Arc::new(ComputerUseAuthorityRegistryV1::default());
    let grant = approved_grant(&caller, unix_time_ms());
    register_session(&registry, &caller, &grant, false);
    let operations = Arc::new(CancelDuringDispatch {
        cleanups: AtomicUsize::new(0),
    });
    let host = RegistryBoundComputerUseMcpHostV1::new(
        registry.clone(),
        ComputerUseConsentBrokerV1::default(),
        operations.clone(),
    );
    let result = host.handle(
        &caller,
        Request::Observe {
            session_id: caller.identity.session_id.clone(),
            observation: ComputerUseObservationKindV1::Screenshot,
        },
        &AtomicBool::new(false),
    );

    assert_eq!(result.status, Status::Denied);
    assert_eq!(result.reason_code.as_deref(), Some("cancelled"));
    assert_eq!(operations.cleanups.load(Ordering::Relaxed), 1);
    assert!(matches!(
        registry.authorize(
            &caller.identity,
            caller.actor,
            &caller.role_id,
            unix_time_ms()
        ),
        Err(crate::computer_use::guest_protocol::GuestProtocolErrorV1::StaleLease)
    ));
}
