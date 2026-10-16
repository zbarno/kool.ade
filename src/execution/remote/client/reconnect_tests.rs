use super::*;
use crate::execution::remote::protocol::{EnvelopeV1, SYSTEM_IMAGE_SESSION_ID_V1};
use std::{
    collections::VecDeque,
    sync::{Arc, Mutex},
};

struct FakeTransport {
    generation: u8,
    sent: Arc<Mutex<Vec<(u8, EnvelopeV1)>>>,
    responses: VecDeque<EnvelopeV1>,
}

impl RemoteRpcTransport for FakeTransport {
    fn send(&mut self, request: &EnvelopeV1) -> std::io::Result<()> {
        self.sent
            .lock()
            .unwrap()
            .push((self.generation, request.clone()));
        let payload = if request.kind == Kind::Health {
            serde_json::json!({"wire_version":PROTOCOL_VERSION_V1,"probe":healthy_probe()})
        } else {
            serde_json::Value::Null
        };
        self.responses.push_back(EnvelopeV1 {
            version: request.version,
            request_id: request.request_id.clone(),
            execution_id: request.execution_id.clone(),
            sequence: request.sequence,
            kind: Kind::Acknowledge,
            payload,
        });
        Ok(())
    }

    fn receive(&mut self) -> std::io::Result<Option<EnvelopeV1>> {
        Ok(self.responses.pop_front())
    }

    fn terminate(&mut self) -> std::io::Result<()> {
        Ok(())
    }
    fn diagnostic_stderr(&self) -> String {
        String::new()
    }
}

fn healthy_probe() -> crate::execution::backend::BackendProbeV1 {
    use crate::execution::{
        backend::{BackendCapabilitiesV1, BackendProbeV1},
        contract::ResourceBudgetV1,
    };
    BackendProbeV1 {
        wire_version: PROTOCOL_VERSION_V1,
        available: true,
        compatible: true,
        architecture: Some("amd64".into()),
        failure: None,
        capabilities: Some(BackendCapabilitiesV1 {
            app_version: env!("CARGO_PKG_VERSION").into(),
            docker_version: Some("27.0".into()),
            docker_api_version: Some("1.53".into()),
            docker_rootless: Some(true),
            docker_desktop: Some(false),
            linux_containers: Some(true),
            seccomp_enabled: Some(true),
            memory_limit: Some(true),
            cpu_quota: Some(true),
            pids_limit: Some(true),
            disk_available_bytes: Some(1_000_000),
            current_execution_count: 0,
            maximum_execution_count: Some(1),
            resource_ceiling: ResourceBudgetV1 {
                cpu_millis: 1000,
                memory_bytes: 1024 * 1024,
                process_count: 16,
                temporary_disk_bytes: 1024 * 1024,
                output_bytes: 1024,
            },
            trusted_image_digests: vec!["sha256:approved".into()],
            policy_hash: crate::execution::policy::policy_hash(),
        }),
    }
}

/// Production boundary: controller reconnect rotates the session key and retains
/// its authenticated RPC transport. This proves Health and the next signed image
/// request use the same client transport; signer unit tests cannot catch a
/// transport swap between those operations. Fake transport only, no SSH, Docker,
/// or network dependency (low setup cost).
#[test]
fn reconnect_keeps_bootstrap_and_authorized_image_rpc_on_the_same_client_transport() {
    use crate::execution::{
        contract::WorkspaceRef,
        docker::image_extension::{
            LinuxDistribution, PackageTrust, SystemImageExtensionRequest,
            builder::PackageArtifactIdentity,
        },
    };
    let sent = Arc::new(Mutex::new(Vec::new()));
    let mut client = RemoteWorkerClient {
        config: crate::execution::remote::ssh::SshWorkerConfig {
            stable_id: "test-worker".into(),
            host_alias: "worker".into(),
            ssh_principal: "builder".into(),
            verified_host_key_fingerprint: "SHA256:AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA"
                .into(),
            known_hosts_path: std::path::PathBuf::new(),
            ssh_binary: std::path::PathBuf::new(),
        },
        transport: Box::new(FakeTransport {
            generation: 1,
            sent: Arc::clone(&sent),
            responses: VecDeque::new(),
        }),
        worker_id: "test-worker".into(),
        probe: unavailable_probe(),
        sequences: HashMap::new(),
        last_health_check: Instant::now(),
        system_image_capability_key: [0x22; 32],
    };
    client
        .reconnect_and_negotiate_with(
            || {
                Ok(Box::new(FakeTransport {
                    generation: 2,
                    sent: Arc::clone(&sent),
                    responses: VecDeque::new(),
                }))
            },
            false,
        )
        .unwrap();

    let workspace = WorkspaceRef {
        task_id: "00000000-0000-4000-8000-000000000137".into(),
        project_id: "00000000-0000-4000-8000-000000000138".into(),
        source_sha: "a".repeat(40),
    };
    let proposal = SystemImageExtensionRequest {
        task_id: workspace.task_id.clone(),
        project_id: workspace.project_id.clone(),
        distribution: LinuxDistribution::DebianBookworm,
        base_profile: crate::execution::docker::images::RuntimeProfile::General,
        base_image_digest: format!("sha256:{}", "a".repeat(64)),
        target_arch: "x86_64".into(),
        packages: vec!["curl".into()],
        trust: PackageTrust::ApprovedDistributionRepository,
        purpose: "approved task requirement".into(),
    };
    let manifest = vec![PackageArtifactIdentity {
        name: "curl".into(),
        version: "1.0-1".into(),
        architecture: "amd64".into(),
        source: "https://deb.debian.org/debian/pool/main/c/curl/curl_1.0-1_amd64.deb".into(),
        sha256: "b".repeat(64),
    }];
    let transfer_id = "00000000-0000-4000-8000-000000000139";
    let decision = crate::harness::DependencyDecision::UserAuthorizeForTask;
    let capability = client
        .system_image_capability(transfer_id, &proposal, decision, &workspace, &manifest)
        .unwrap();
    let request = EnvelopeV1 {
        version: PROTOCOL_VERSION_V1,
        request_id: uuid::Uuid::new_v4().to_string(),
        execution_id: "00000000-0000-4000-8000-000000000000".into(),
        sequence: 1,
        kind: Kind::BeginSystemImageTransfer,
        payload: serde_json::json!({"transfer_id":transfer_id,"capability":capability}),
    };
    assert_eq!(
        client.send_envelope(&request).unwrap().kind,
        Kind::Acknowledge
    );
    let sent = sent.lock().unwrap();
    assert_eq!(sent.len(), 2);
    assert!(sent.iter().all(|(generation, _)| *generation == 2));
    assert_eq!(sent[0].1.execution_id, SYSTEM_IMAGE_SESSION_ID_V1);
    assert_eq!(sent[0].1.kind, Kind::Health);
    assert_eq!(sent[1].1.kind, Kind::BeginSystemImageTransfer);
    let key_hex = sent[0].1.payload["system_image_key"].as_str().unwrap();
    let key: [u8; 32] = (0..32)
        .map(|index| u8::from_str_radix(&key_hex[index * 2..index * 2 + 2], 16).unwrap())
        .collect::<Vec<_>>()
        .try_into()
        .unwrap();
    assert!(
        crate::execution::docker::image_extension::verify_system_image_capability(
            &key,
            sent[1].1.payload["capability"].as_str().unwrap(),
            transfer_id,
            &proposal,
            decision,
            &workspace,
            &manifest,
        )
    );
}
