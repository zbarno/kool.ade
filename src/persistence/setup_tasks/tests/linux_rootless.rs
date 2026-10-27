use crate::app::setup_attention::catalog::{
    EvidenceCode as Code, EvidenceOrigin as Origin, EvidenceState as State, FindingStatus,
    HostKind, ImageArchitectureV1, ImageProfileV1, PrerequisiteCapability as Capability,
    PrerequisiteDomain as Domain, PrerequisiteId, PrerequisiteSubject as Subject,
    ProbeEvidenceV1 as Evidence, ProbeObservationV1 as Observation,
};

use super::super::{
    AttemptState, AuthorizationBindingV1, SetupTaskStateV1, SetupTaskStoreV1,
    authorization_scope_digest,
};
use super::support::{ready_store, scan, snapshot};

mod smoke_task;

const OPERATION_ID: &str = "00000000-0000-4000-8000-000000000001";
const RECEIPT_ID: &str = "00000000-0000-4000-8000-000000000002";
const OWNER_ID: &str = "00000000-0000-4000-8000-000000000004";

fn awaiting_task(task_id: &str) -> SetupTaskStoreV1 {
    let mut store = ready_store(task_id);
    let scopes = vec![task_id.to_owned()];
    store
        .request_attempt(task_id, OPERATION_ID.into(), 1, scopes.clone(), OWNER_ID, 2)
        .unwrap();
    store
        .authorize_attempt(
            task_id,
            OPERATION_ID,
            AuthorizationBindingV1 {
                receipt_id: RECEIPT_ID.into(),
                scope_digest: authorization_scope_digest(task_id, OPERATION_ID, 1, &scopes),
                authorized_at_unix_ms: 3,
            },
            OWNER_ID,
            3,
        )
        .unwrap();
    store
        .await_verification(task_id, OPERATION_ID, OWNER_ID, 4)
        .unwrap();
    store
}

fn host_fact(
    generation: u64,
    capability: Capability,
    code: Code,
    observation: Option<Observation>,
) -> Evidence {
    Evidence {
        prerequisite: PrerequisiteId {
            domain: Domain::Host,
            platform: HostKind::Linux,
            capability,
            subject: Subject::None,
        },
        state: State::Ready,
        code,
        origin: Origin::HostRuntime,
        scan_generation: generation,
        source_generation: None,
        observed_at_unix_ms: generation * 10,
        observation,
    }
}

fn rootless_smoke_fact(generation: u64) -> Evidence {
    let image = crate::execution::docker::images::ImageLock::bundled()
        .unwrap()
        .select(
            crate::execution::docker::images::RuntimeProfile::General,
            "amd64",
        )
        .unwrap();
    Evidence {
        prerequisite: PrerequisiteId {
            domain: Domain::Docker,
            platform: HostKind::Linux,
            capability: Capability::DockerRootlessSmoke,
            subject: Subject::Image {
                profile: ImageProfileV1::General,
                architecture: ImageArchitectureV1::Amd64,
            },
        },
        state: State::Ready,
        code: Code::ApprovedImageSmokeSucceeded,
        origin: Origin::ApprovedImageSmokeTest,
        scan_generation: generation,
        source_generation: None,
        observed_at_unix_ms: generation * 10,
        observation: Some(Observation::DockerRootlessSmoke {
            endpoint_fingerprint: "a".repeat(64),
            daemon_fingerprint: "b".repeat(64),
            daemon_version: "27.1.2".into(),
            image_reference: image.reference,
            image_digest: image.digest,
            image_platform: image.platform,
            completed_at_unix_ms: generation * 10,
        }),
    }
}

fn reconcile_without_findings(
    store: &mut SetupTaskStoreV1,
    domains: &[&str],
    evidence: Vec<Evidence>,
) {
    let generation = store.latest_scan_generation + 1;
    let mut current = snapshot(&[], domains, generation * 10);
    current.probe_evidence = evidence;
    scan(store, &current).unwrap();
}

#[test]
fn package_attempt_reconciles_from_fresh_rootless_helper_evidence() {
    let task_id = "setup:linux:rootless:packages";
    let mut store = awaiting_task(task_id);
    let stale_helpers = host_fact(
        store.latest_scan_generation,
        Capability::LinuxRootlessHelpers,
        Code::LinuxHostCheckReady,
        Some(Observation::LinuxRootlessHelpers {
            newuidmap: true,
            newgidmap: true,
            rootlesskit: true,
            dockerd_rootless: true,
            setuptool: true,
            slirp4netns: Some(true),
        }),
    );
    reconcile_without_findings(&mut store, &[], vec![stale_helpers]);
    assert_eq!(
        store.tasks[task_id].attempts[0].state,
        AttemptState::AwaitingVerification
    );

    let generation = store.latest_scan_generation + 1;
    let helpers = host_fact(
        generation,
        Capability::LinuxRootlessHelpers,
        Code::LinuxHostCheckReady,
        Some(Observation::LinuxRootlessHelpers {
            newuidmap: true,
            newgidmap: true,
            rootlesskit: true,
            dockerd_rootless: true,
            setuptool: true,
            slirp4netns: Some(true),
        }),
    );

    reconcile_without_findings(&mut store, &[], vec![helpers]);

    let task = &store.tasks[task_id];
    assert_eq!(task.state, SetupTaskStateV1::Verified);
    assert_eq!(task.attempts[0].state, AttemptState::Verified);
    assert_eq!(
        task.verified_result.as_ref().unwrap().evidence_domain,
        "host:linux:rootless_helpers"
    );
    store.validate().unwrap();
}

#[test]
fn subordinate_id_attempt_reconciles_from_current_uid_and_gid_ranges() {
    let task_id = "setup:linux:rootless:subids";
    let mut store = awaiting_task(task_id);
    let generation = store.latest_scan_generation + 1;
    let facts = vec![
        host_fact(
            generation,
            Capability::LinuxSubuid,
            Code::LinuxHostCheckReady,
            Some(Observation::LinuxSubordinateIds { configured: true }),
        ),
        host_fact(
            generation,
            Capability::LinuxSubgid,
            Code::LinuxHostCheckReady,
            Some(Observation::LinuxSubordinateIds { configured: true }),
        ),
    ];

    reconcile_without_findings(&mut store, &[], facts);

    let task = &store.tasks[task_id];
    assert_eq!(task.state, SetupTaskStateV1::Verified);
    assert_eq!(task.attempts[0].state, AttemptState::Verified);
    assert_eq!(
        task.verified_result.as_ref().unwrap().evidence_domain,
        "host:linux:subordinate_ids"
    );
    store.validate().unwrap();
}

#[test]
fn engine_readiness_or_image_inventory_cannot_verify_rootless_summary_or_service() {
    for task_id in ["setup:docker:rootless", "setup:linux:rootless:service"] {
        let mut store = awaiting_task(task_id);
        reconcile_without_findings(&mut store, &["docker", "images:general:amd64"], Vec::new());
        assert_eq!(
            store.tasks[task_id].attempts[0].state,
            AttemptState::AwaitingVerification,
            "engine and inventory cannot complete {task_id}"
        );
        assert_eq!(
            store.tasks[task_id].state,
            SetupTaskStateV1::AwaitingVerification,
            "engine and inventory cannot complete {task_id}"
        );

        reconcile_without_findings(&mut store, &["docker:rootless-smoke"], Vec::new());
        assert_eq!(
            store.tasks[task_id].attempts[0].state,
            AttemptState::AwaitingVerification,
            "a checked-domain label without typed smoke evidence cannot complete {task_id}"
        );
        let generation = store.latest_scan_generation + 1;
        reconcile_without_findings(
            &mut store,
            &["docker:rootless-smoke"],
            vec![rootless_smoke_fact(generation)],
        );

        let task = &store.tasks[task_id];
        assert_eq!(task.state, SetupTaskStateV1::Verified);
        assert_eq!(task.attempts[0].state, AttemptState::Verified);
        assert_eq!(
            task.verified_result.as_ref().unwrap().evidence_domain,
            "docker:rootless-smoke"
        );
        store.validate().unwrap();
    }
}

#[test]
fn rootless_summary_does_not_block_the_first_repair_task() {
    let mut store = SetupTaskStoreV1::default();
    let initial = snapshot(
        &[
            (
                "setup:docker:rootless",
                crate::app::setup_attention::catalog::FindingStatus::NeedsAttention,
                &[],
            ),
            (
                "setup:linux:rootless:packages",
                crate::app::setup_attention::catalog::FindingStatus::NeedsAttention,
                &[],
            ),
            (
                "setup:linux:rootless:subids",
                crate::app::setup_attention::catalog::FindingStatus::NeedsAttention,
                &["setup:linux:rootless:packages"],
            ),
            (
                "setup:linux:rootless:service",
                crate::app::setup_attention::catalog::FindingStatus::NeedsAttention,
                &[
                    "setup:linux:rootless:packages",
                    "setup:linux:rootless:subids",
                ],
            ),
        ],
        &[],
        10,
    );
    scan(&mut store, &initial).unwrap();

    assert_eq!(
        store.tasks["setup:linux:rootless:packages"].state,
        SetupTaskStateV1::ReadyToFix
    );
    assert!(
        store.tasks["setup:linux:rootless:packages"]
            .blocked_by
            .is_empty()
    );
    assert_eq!(
        store.tasks["setup:linux:rootless:subids"].blocked_by,
        vec!["setup:linux:rootless:packages"]
    );
    assert_eq!(
        store.tasks["setup:linux:rootless:service"].blocked_by,
        vec![
            "setup:linux:rootless:packages",
            "setup:linux:rootless:subids"
        ]
    );
}
