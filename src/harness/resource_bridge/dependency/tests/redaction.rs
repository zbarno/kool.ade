use super::super::{decision_allowed, triage::triage, validation::redact_and_bound};
use super::npm_add;
use crate::harness::{DependencyDecision, DependencyKind};

#[test]
fn request_redaction_hides_url_query_credentials_and_rejects_the_request() {
    let secret = "synthetic-query-token";
    let mut need = npm_add(
        Some(&format!("HTTPS://packages.example.net/?token={secret}")),
        &format!("npm install zod@^4.0.0 --registry=HtTpS://packages.example.net/?token={secret}"),
        "^4.0.0",
    );
    need.reason = format!("Restore dependencies from HTTP://packages.example.net/#key={secret}");

    let request = triage(Some("task-1"), need);

    assert_eq!(request.decision, DependencyDecision::Reject);
    assert_eq!(
        request.status,
        crate::harness::DependencyRequestStatus::Failed
    );
    let serialized = serde_json::to_string(&request.need).unwrap();
    assert!(!serialized.contains(secret));
    assert!(serialized.contains("?[REDACTED]"));
}

#[test]
fn request_redaction_keeps_ipv6_url_queries_private() {
    let ipv6_secret = "synthetic-ipv6-token";
    let comma_secret = "synthetic-comma-token";
    let mut need = npm_add(
        Some(&format!("https://[2001:db8::1]/?token={ipv6_secret}")),
        "npm install zod@^4.0.0",
        "^4.0.0",
    );
    need.reason =
        format!("Use the private registry https://example.net/?token=abc,signature={comma_secret}");

    redact_and_bound(&mut need);

    let serialized = serde_json::to_string(&need).unwrap();
    assert!(!serialized.contains(ipv6_secret));
    assert!(!serialized.contains(comma_secret));
    assert!(serialized.contains("https://[2001:db8::1]/?[REDACTED]"));
    assert!(serialized.contains("https://example.net/?[REDACTED]"));
}

#[test]
fn scheme_less_registry_query_is_redacted_before_manager_triage() {
    let secret = "synthetic-scheme-less-token";
    let need = npm_add(
        Some(&format!("registry.npmjs.org?token={secret}")),
        "npm install zod@^4.0.0",
        "^4.0.0",
    );

    let request = triage(Some("task-1"), need);

    assert_eq!(request.decision, DependencyDecision::Reject);
    assert_eq!(
        request.status,
        crate::harness::DependencyRequestStatus::Failed
    );
    let serialized = serde_json::to_string(&request.need).unwrap();
    assert!(!serialized.contains(secret));
    assert!(serialized.contains("registry.npmjs.org?[REDACTED]"));
}

#[test]
fn uppercase_scheme_userinfo_is_redacted_before_manager_triage() {
    let username = "synthetic-registry-user";
    let password = "synthetic-registry-password";
    let need = npm_add(
        Some(&format!(
            "HTTPS://{username}:{password}@packages.example.net/"
        )),
        "npm install zod@^4.0.0",
        "^4.0.0",
    );

    let request = triage(Some("task-1"), need);

    let serialized = serde_json::to_string(&request.need).unwrap();
    assert!(!serialized.contains(username));
    assert!(!serialized.contains(password));
    assert!(serialized.contains("HTTPS://[REDACTED]@packages.example.net/"));
}

#[test]
fn overlong_raw_dependency_command_is_rejected_before_truncation() {
    let prefix = "npm install zod@^4.0.0";
    let command = format!(
        "{prefix}{}; npm install left-pad@1.0.0",
        " ".repeat(2_048 - prefix.len())
    );
    let mut need = npm_add(Some("https://registry.npmjs.org"), prefix, "^4.0.0");
    need.command = command;

    let request = triage(Some("task-1"), need);

    assert_eq!(request.decision, DependencyDecision::Reject);
    assert_eq!(
        request.status,
        crate::harness::DependencyRequestStatus::Failed
    );
    assert!(request.need.command.len() <= 2_048);
}

#[test]
fn npm_save_mode_must_match_the_requested_dependency_scope() {
    let mut dev_as_production = npm_add(
        Some("https://registry.npmjs.org"),
        "npm install zod@^4.0.0 --save-prod",
        "^4.0.0",
    );
    dev_as_production.kind = DependencyKind::DevelopmentDependency;
    assert!(!decision_allowed(
        &dev_as_production,
        DependencyDecision::UserAuthorizeForTask
    ));

    let mut production_as_dev = npm_add(
        Some("https://registry.npmjs.org"),
        "npm install zod@^4.0.0 --save-dev",
        "^4.0.0",
    );
    assert!(!decision_allowed(
        &production_as_dev,
        DependencyDecision::UserAuthorizeForTask
    ));
    production_as_dev.kind = DependencyKind::DevelopmentDependency;
    assert!(decision_allowed(
        &production_as_dev,
        DependencyDecision::UserAuthorizeForTask
    ));
}

#[test]
fn request_bounds_preserve_utf8_character_boundaries() {
    let mut need = npm_add(None, &format!("{}é", "x".repeat(2_047)), "latest");
    need.reason = format!("{}é", "r".repeat(2_047));
    need.package = Some(format!("{}é", "p".repeat(255)));
    need.version = Some(format!("{}é", "v".repeat(255)));
    need.source = Some(format!("{}é", "s".repeat(2_047)));

    redact_and_bound(&mut need);

    for (value, max) in [
        (&need.command, 2_048),
        (&need.reason, 2_048),
        (need.package.as_ref().unwrap(), 256),
        (need.version.as_ref().unwrap(), 256),
        (need.source.as_ref().unwrap(), 2_048),
    ] {
        assert!(value.len() <= max);
        assert!(value.is_char_boundary(value.len()));
    }
}
