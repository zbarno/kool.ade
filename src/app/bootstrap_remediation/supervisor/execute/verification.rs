use std::time::SystemTime;

use crate::{
    app::bootstrap_remediation::{
        plan::RemediationPlan,
        supervisor::{ReadinessVerifier, VerificationReceipt},
    },
    persistence::setup_tasks::{
        RemediationVerificationStatusV1 as Status, RemediationVerificationV1,
    },
};

pub(super) struct VerificationCheck {
    pub(super) persisted: RemediationVerificationV1,
    pub(super) receipt: Option<VerificationReceipt>,
}

pub(super) fn check_verification(
    verifier: &dyn ReadinessVerifier,
    plan: &RemediationPlan,
    launched_at_unix_ms: u64,
    fallback_now: u64,
) -> VerificationCheck {
    let verification_started_at_unix_ms = current_unix_ms(fallback_now).max(launched_at_unix_ms);
    let Ok(receipt) = verifier.verify(plan) else {
        return unavailable(plan, current_unix_ms(fallback_now));
    };
    let now = current_unix_ms(fallback_now);
    if validate_receipt(
        plan,
        &receipt,
        launched_at_unix_ms,
        verification_started_at_unix_ms,
        now,
    )
    .is_err()
    {
        return unavailable(plan, now);
    }
    let status = if receipt.ready {
        Status::Ready
    } else {
        Status::NotReady
    };
    VerificationCheck {
        persisted: RemediationVerificationV1 {
            status,
            probe_ids: receipt.probe_ids.clone(),
            observed_at_unix_ms: receipt.observed_at_unix_ms,
            evidence_digest: Some(receipt.evidence_digest.clone()),
        },
        receipt: Some(receipt),
    }
}

fn current_unix_ms(fallback: u64) -> u64 {
    SystemTime::now()
        .duration_since(SystemTime::UNIX_EPOCH)
        .map(|duration| duration.as_millis() as u64)
        .unwrap_or(fallback)
}

fn unavailable(plan: &RemediationPlan, observed_at_unix_ms: u64) -> VerificationCheck {
    VerificationCheck {
        persisted: RemediationVerificationV1 {
            status: Status::Unavailable,
            probe_ids: plan.verification_probe_ids.clone(),
            observed_at_unix_ms,
            evidence_digest: None,
        },
        receipt: None,
    }
}

fn validate_receipt(
    plan: &RemediationPlan,
    receipt: &VerificationReceipt,
    launched_at_unix_ms: u64,
    verification_started_at_unix_ms: u64,
    now_unix_ms: u64,
) -> anyhow::Result<()> {
    let actual = receipt
        .probe_ids
        .iter()
        .collect::<std::collections::BTreeSet<_>>();
    let required = plan
        .verification_probe_ids
        .iter()
        .collect::<std::collections::BTreeSet<_>>();
    anyhow::ensure!(actual == required, "fresh verification probe set changed");
    anyhow::ensure!(
        receipt.observed_at_unix_ms >= launched_at_unix_ms
            && receipt.observed_at_unix_ms > verification_started_at_unix_ms
            && receipt.observed_at_unix_ms <= now_unix_ms
            && now_unix_ms - receipt.observed_at_unix_ms <= 5 * 60 * 1000,
        "readiness verification predates the operation, predates the fresh probe, or is stale"
    );
    anyhow::ensure!(
        receipt.evidence_digest.len() == 64
            && receipt
                .evidence_digest
                .bytes()
                .all(|byte| byte.is_ascii_hexdigit()),
        "readiness verification receipt is invalid"
    );
    Ok(())
}

pub(super) fn sanitize_diagnostic(value: &str) -> Option<String> {
    let redacted = crate::error::redact_secrets(value);
    let redacted = crate::app::bootstrap_remediation::redact_keyed_values(&redacted);
    let mut bounded = String::new();
    for character in redacted
        .chars()
        .filter(|character| !character.is_control() || *character == '\n' || *character == '\t')
    {
        if bounded.len() + character.len_utf8() > 4000 {
            break;
        }
        bounded.push(character);
    }
    if bounded.trim().is_empty() {
        return None;
    }
    if redacted.chars().count() > bounded.chars().count() {
        bounded.push_str("…[truncated]");
    }
    Some(bounded)
}

#[cfg(test)]
mod tests {
    use super::sanitize_diagnostic;
    use crate::app::{
        bootstrap_remediation::{
            identity::HostIdentity,
            plan::RemediationPlan,
            request::RemediationRequest,
            supervisor::{ReadinessVerifier, VerificationReceipt},
        },
        bootstrap_repair_policy::{Host, Operation, Proposal},
    };

    fn test_plan() -> RemediationPlan {
        let identity =
            HostIdentity::from_facts(Host::Linux, "6.12", "test-machine", "test-user").unwrap();
        let request = RemediationRequest::from_proposal_for_test(
            &Proposal {
                schema_version: 1,
                operation: Operation::PrepareLockedImage,
                host: Host::Linux,
                prerequisite_id: format!("setup:images:general:amd64:{}", "a".repeat(64)),
                image_profile: Some("general".into()),
            },
            &identity,
        )
        .unwrap();
        RemediationPlan::build_for_test(&request, &identity).unwrap()
    }

    #[test]
    fn persisted_diagnostics_are_bounded_and_remove_credentials() {
        let diagnostic = sanitize_diagnostic(
            "token=synthetic-secret Authorization: Bearer second-secret\npassword: third-secret",
        )
        .unwrap();
        assert!(!diagnostic.contains("synthetic-secret"));
        assert!(!diagnostic.contains("second-secret"));
        assert!(!diagnostic.contains("third-secret"));
        assert!(diagnostic.len() <= 4096);
        assert!(sanitize_diagnostic(&"x".repeat(20_000)).unwrap().len() <= 4096);
    }

    #[test]
    fn persisted_diagnostics_redact_quoted_json_yaml_and_camel_case_secrets() {
        let diagnostic = sanitize_diagnostic(
            r#"{"token" : "json-token"}
'client_secret': 'yaml-secret'
clientSecret: client-secret
accessToken: access-secret
refreshToken: refresh-camel-secret
privateKey: private-secret
apiKey: camel-secret
secretAccessKey: compound-secret
AWS_SECRET_ACCESS_KEY=aws-secret
GITHUB_TOKEN=github-token
OPENAI_API_KEY: "openai-api-key"
AZURE_CLIENT_SECRET: azure-client-secret
DB_PASSWORD: database password with spaces
password: 'can''t share synthetic-single-quote-secret'
DB_PASSWORD=synthetic-equals-first-word synthetic-equals-secret-tail
https://storage.invalid/blob?sig=synthetic-sas-signature&sv=2025
https://s3.invalid/object?X-Amz-Signature=synthetic-aws-signature&X-Amz-Credential=synthetic-access-key
Authorization: Bearer auth-secret
password: plain-secret with spaces
refresh_token="refresh-secret""#,
        )
        .unwrap();
        for secret in [
            "json-token",
            "yaml-secret",
            "client-secret",
            "access-secret",
            "refresh-camel-secret",
            "private-secret",
            "camel-secret",
            "compound-secret",
            "aws-secret",
            "github-token",
            "openai-api-key",
            "azure-client-secret",
            "database password with spaces",
            "synthetic-single-quote-secret",
            "synthetic-equals-first-word",
            "synthetic-equals-secret-tail",
            "synthetic-sas-signature",
            "synthetic-aws-signature",
            "synthetic-access-key",
            "auth-secret",
            "plain-secret",
            "with spaces",
            "refresh-secret",
        ] {
            assert!(!diagnostic.contains(secret), "leaked {secret}");
        }
    }

    #[test]
    fn pre_operation_readiness_receipt_is_not_accepted_as_fresh() {
        struct CachedReceipt(VerificationReceipt);
        impl ReadinessVerifier for CachedReceipt {
            fn verify(&self, _plan: &RemediationPlan) -> anyhow::Result<VerificationReceipt> {
                Ok(self.0.clone())
            }
        }

        let plan = test_plan();
        let launched_at = crate::app::bootstrap_remediation::approval::now_unix_ms().unwrap();
        let verifier = CachedReceipt(VerificationReceipt {
            probe_ids: plan.verification_probe_ids.clone(),
            observed_at_unix_ms: launched_at.saturating_sub(1),
            evidence_digest: "a".repeat(64),
            ready: true,
        });
        let check =
            super::check_verification(&verifier, &plan, launched_at, launched_at.saturating_add(1));

        assert_eq!(check.persisted.status, super::Status::Unavailable);
        assert!(check.receipt.is_none());
    }

    #[test]
    fn readiness_observed_during_operation_is_too_old_for_post_operation_probe() {
        let plan = test_plan();
        let receipt = VerificationReceipt {
            probe_ids: plan.verification_probe_ids.clone(),
            observed_at_unix_ms: 200,
            evidence_digest: "a".repeat(64),
            ready: true,
        };
        assert!(super::validate_receipt(&plan, &receipt, 100, 300, 400).is_err());
    }

    #[test]
    fn same_millisecond_readiness_is_not_proven_to_follow_the_probe_request() {
        let plan = test_plan();
        let receipt = VerificationReceipt {
            probe_ids: plan.verification_probe_ids.clone(),
            observed_at_unix_ms: 300,
            evidence_digest: "a".repeat(64),
            ready: true,
        };
        assert!(super::validate_receipt(&plan, &receipt, 100, 300, 400).is_err());
    }
}
