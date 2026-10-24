use super::*;
use std::sync::{Arc, Mutex};

struct LostReplyTransport {
    sent: Arc<Mutex<Vec<EnvelopeV1>>>,
    terminated: Arc<Mutex<bool>>,
}

impl crate::execution::remote::transport::RemoteRpcTransport for LostReplyTransport {
    fn send(&mut self, envelope: &EnvelopeV1) -> std::io::Result<()> {
        self.sent.lock().unwrap().push(envelope.clone());
        Ok(())
    }

    fn receive(&mut self) -> std::io::Result<Option<EnvelopeV1>> {
        Err(std::io::Error::new(
            std::io::ErrorKind::UnexpectedEof,
            "synthetic lost Start reply",
        ))
    }

    fn terminate(&mut self) -> std::io::Result<()> {
        *self.terminated.lock().unwrap() = true;
        Ok(())
    }

    fn diagnostic_stderr(&self) -> String {
        String::new()
    }
}

/// Production boundary: a grant-bound Start crosses the controller's RPC
/// transport and can fail after its frame is written. A grant-store unit test
/// cannot prove the exchange layer avoids replay after that lost reply. The
/// fake transport needs no SSH, Docker, or network service (low setup cost).
#[test]
fn lost_grant_bound_start_reply_is_not_reconnected_or_replayed() {
    let sent = Arc::new(Mutex::new(Vec::new()));
    let terminated = Arc::new(Mutex::new(false));
    let mut client = RemoteWorkerClient {
        config: crate::execution::remote::ssh::SshWorkerConfig {
            stable_id: "worker-start-reply-loss".into(),
            host_alias: "worker".into(),
            ssh_principal: "builder".into(),
            verified_host_key_fingerprint: "SHA256:AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA"
                .into(),
            known_hosts_path: std::path::PathBuf::new(),
            ssh_binary: std::path::PathBuf::new(),
        },
        transport: Box::new(LostReplyTransport {
            sent: Arc::clone(&sent),
            terminated: Arc::clone(&terminated),
        }),
        worker_id: "worker-start-reply-loss".into(),
        probe: unavailable_probe(),
        sequences: HashMap::new(),
        last_health_check: Instant::now(),
        system_image_capability_key: zeroize::Zeroizing::new([0x22; 32]),
    };
    let execution_id = "00000000-0000-4000-8000-000000000141";
    let grant_use = crate::persistence::remote_private_file_grants::lock_authorized_grants(
        std::path::Path::new("/unused-for-empty-grant-set"),
        &[],
        0,
    )
    .unwrap();
    let request = ExchangeRequest {
        kind: Kind::Start,
        execution_id,
        request_id: execution_id,
        sequence: 1,
        payload: serde_json::json!({"request":"synthetic-start"}),
        start_grant_use: Some(grant_use),
    };
    let mut reconnects = 0;
    let mut journaled = Vec::new();
    let error = client
        .exchange_with_reconnect_identity_using(
            request,
            &mut |_| {
                reconnects += 1;
                Ok(())
            },
            |worker_id, envelope| {
                journaled.push((worker_id.to_owned(), envelope.clone()));
                Ok(())
            },
            |_, _| {},
        )
        .unwrap_err();

    assert_eq!(error.kind, ExecutionErrorKind::WorkerDisconnected);
    assert_eq!(reconnects, 0);
    assert_eq!(journaled.len(), 1);
    assert_eq!(journaled[0].0, "worker-start-reply-loss");
    assert_eq!(journaled[0].1.kind, Kind::Start);
    assert_eq!(journaled[0].1.request_id, execution_id);
    let sent = sent.lock().unwrap();
    assert_eq!(sent.len(), 1);
    assert_eq!(sent[0].kind, Kind::Start);
    assert!(*terminated.lock().unwrap());
}
