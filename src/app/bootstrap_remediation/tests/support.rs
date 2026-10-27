use super::super::*;
use crate::{
    app::bootstrap_remediation::supervisor::{
        ElevationReceipt, NativeCommandRunner, NativeElevation, OperationRegistry,
        PreconditionReceipt, PreconditionVerifier, ProcessOutput, ProcessRunError,
        ProcessRunFailure, ReadinessVerifier, TrustedInvocation, TrustedProgram,
        VerificationReceipt,
    },
    app::bootstrap_repair_policy::{Host, Operation, Proposal},
    persistence::setup_tasks::{SetupTaskEventV1, SetupTaskStateV1, SetupTaskStoreV1, SetupTaskV1},
};
use std::collections::BTreeMap;

pub(super) fn plan(host: Host, privilege: Option<PrivilegeLevel>) -> RemediationPlan {
    let proposal = Proposal {
        schema_version: 1,
        operation: Operation::PrepareLockedImage,
        host,
        prerequisite_id: format!("setup:images:general:amd64:{}", "a".repeat(64)),
        image_profile: Some("general".into()),
    };
    let identity =
        HostIdentity::from_facts(host, "10.2.0", "synthetic-machine", "synthetic-user").unwrap();
    let request = RemediationRequest::from_proposal_for_test(&proposal, &identity).unwrap();
    let mut plan = RemediationPlan::build_for_test(&request, &identity).unwrap();
    if let Some(privilege) = privilege {
        plan.set_privilege_for_test(privilege);
        plan.validate().unwrap();
    }
    plan
}

pub(super) fn rootless_packages_plan() -> (RemediationPlan, HostIdentity) {
    let identity = HostIdentity::from_facts(
        Host::Linux,
        "ubuntu-24.04-amd64-kernel-6.8.0",
        "synthetic-machine",
        "1000",
    )
    .unwrap();
    let proposal = Proposal {
        schema_version: 1,
        operation: Operation::InstallLinuxRootlessPackages,
        host: Host::Linux,
        prerequisite_id: "setup:linux:rootless:packages".into(),
        image_profile: None,
    };
    let request = RemediationRequest::from_proposal_for_test(&proposal, &identity).unwrap();
    let plan = RemediationPlan::build_for_test(&request, &identity).unwrap();
    (plan, identity)
}

pub(super) fn ready_store(task_id: &str) -> SetupTaskStoreV1 {
    SetupTaskStoreV1 {
        schema_version: 2,
        latest_scan_generation: 1,
        tasks: BTreeMap::from([(
            task_id.to_owned(),
            SetupTaskV1 {
                schema_version: 1,
                id: task_id.to_owned(),
                title: "Prepare the approved runtime image".into(),
                explanation: "The reviewed image is unavailable.".into(),
                next_step: "Review the image preparation operation.".into(),
                dependencies: Vec::new(),
                blocked_by: Vec::new(),
                state: SetupTaskStateV1::ReadyToFix,
                created_at_unix_ms: 10,
                observed_at_unix_ms: 10,
                scan_generation: 1,
                verified_at_unix_ms: None,
                verified_result: None,
                attempts: Vec::new(),
                events: vec![SetupTaskEventV1 {
                    observed_at_unix_ms: 10,
                    scan_generation: 1,
                    state: SetupTaskStateV1::ReadyToFix,
                }],
                legacy_source_id: None,
                superseded_by: Vec::new(),
            },
        )]),
        required_image_profiles: Default::default(),
    }
}

pub(super) fn now() -> u64 {
    approval::now_unix_ms().unwrap()
}

pub(super) fn approved(
    plan: &RemediationPlan,
    identity: &HostIdentity,
    store: &mut SetupTaskStoreV1,
    gate: &mut ApprovalGate,
    now: u64,
) -> (AttemptContext, ApprovedRemediation) {
    let owner = "c4d870dd-20a1-44ef-9d3f-73702d5f2222";
    let attempt = AttemptContext::begin_for_test(store, plan, identity, owner, now).unwrap();
    let preview =
        ApprovalPreview::new_for_test(plan.clone(), identity.clone(), attempt.clone()).unwrap();
    let binding = gate
        .grant_at(preview.request_for_test(), identity, now)
        .unwrap();
    attempt
        .persist_authorization_for_test(store, binding.clone(), now + 1)
        .unwrap();
    let approved = gate
        .consume_at(&binding, plan, identity, &attempt, now + 2)
        .unwrap();
    (attempt, approved)
}

pub(super) struct FakeRegistry(pub(super) Host);

impl OperationRegistry for FakeRegistry {
    fn compile(&self, plan: &RemediationPlan) -> anyhow::Result<TrustedInvocation> {
        anyhow::ensure!(plan.host == self.0, "fake platform registry mismatch");
        TrustedInvocation::new(
            self.0,
            TrustedProgram::TestHelper,
            Vec::new(),
            std::time::Duration::from_secs(5),
        )
    }
}

pub(super) struct FreshPreconditions;

impl PreconditionVerifier for FreshPreconditions {
    fn verify(&self, probe_ids: &[String]) -> anyhow::Result<PreconditionReceipt> {
        Ok(PreconditionReceipt {
            probe_ids: probe_ids.to_vec(),
            observed_at_unix_ms: approval::now_unix_ms()?,
            evidence_digest: "c".repeat(64),
            ready: true,
        })
    }
}

pub(super) struct FakeRunner(pub(super) Host);

impl NativeCommandRunner for FakeRunner {
    fn run(
        &self,
        invocation: TrustedInvocation,
        cancellation: &Cancellation,
    ) -> Result<ProcessOutput, ProcessRunError> {
        if invocation.host() != self.0 {
            return Err(ProcessRunError::not_started(
                ProcessRunFailure::FailedBeforeLaunch,
            ));
        }
        if cancellation.is_cancelled() {
            return Err(ProcessRunError::not_started(ProcessRunFailure::Cancelled));
        }
        Ok(ProcessOutput {
            exit_code: 0,
            stdout: "bounded synthetic output".into(),
            stderr: String::new(),
            stdout_truncated: false,
            stderr_truncated: false,
        })
    }

    fn run_elevated(
        &self,
        invocation: TrustedInvocation,
        _elevation: ElevationReceipt,
        cancellation: &Cancellation,
    ) -> Result<ProcessOutput, ProcessRunError> {
        self.run(invocation, cancellation)
    }
}

pub(super) struct FakeElevation {
    pub(super) declined: bool,
}

impl NativeElevation for FakeElevation {
    fn request(
        &self,
        plan: &RemediationPlan,
        identity: &HostIdentity,
        invocation: &TrustedInvocation,
        _cancellation: &Cancellation,
    ) -> anyhow::Result<Option<ElevationReceipt>> {
        if self.declined {
            Ok(None)
        } else {
            Ok(Some(ElevationReceipt::for_test(
                plan, identity, invocation,
            )?))
        }
    }
}

pub(super) struct FreshReadiness;

impl ReadinessVerifier for FreshReadiness {
    fn verify(&self, plan: &RemediationPlan) -> anyhow::Result<VerificationReceipt> {
        std::thread::sleep(std::time::Duration::from_millis(2));
        Ok(VerificationReceipt {
            probe_ids: plan.verification_probe_ids.clone(),
            observed_at_unix_ms: approval::now_unix_ms()?,
            evidence_digest: "a".repeat(64),
            ready: true,
        })
    }
}
