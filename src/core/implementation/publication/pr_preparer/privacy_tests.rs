use super::*;

fn scope_with_private_context(private_context: &str) -> ProposalScope {
    let mut scope = ProposalScope {
        expected: super::super::ExpectedProposal {
            task_uid: "task-privacy".into(),
            base_ref: "refs/heads/main".into(),
            head_ref: "refs/heads/koolade/task-privacy".into(),
            commits: vec!["commit-privacy".into()],
            files: vec!["src/privacy.rs".into()],
            evidence_ids: vec!["evidence-privacy".into()],
        },
        scope_sha256: "a".repeat(64),
        feature_plan: String::new(),
        task_spec: String::new(),
        gate_evidence: String::new(),
        verified_report: String::new(),
        diff: String::new(),
        scope_manifest: String::new(),
    };
    scope.feature_plan = private_context.to_owned();
    scope
}

#[test]
fn public_description_rejects_private_contacts_identifiers_and_context_echoes() {
    for private in [
        "customer@example.invalid",
        "contact number 2025550199",
        "customer record ID patient_case_4f27a981",
        "Customer: Synthetic Cedar Clinic internal billing migration",
        "Customer: Jane Doe",
        "Address: 12 Oak St, Apt 4",
    ] {
        let scope = scope_with_private_context(private);
        let error = validate_public_description(private, &scope).unwrap_err();
        assert!(error.to_string().contains("sensitive value"), "{private}");
    }
}

#[test]
fn public_description_rejects_short_labeled_values_and_partial_address_echoes() {
    for (private, description) in [
        ("Customer: Jane Doe", "Changes requested by Jane Doe."),
        ("Address: 12 Oak St, Apartment 4", "Follow up at 12 Oak St."),
    ] {
        let scope = scope_with_private_context(private);
        assert!(
            validate_public_description(description, &scope).is_err(),
            "description unexpectedly passed for {private}"
        );
    }
}

#[test]
fn public_description_rejects_name_echoes_from_unstructured_context() {
    for (private, description) in [
        (
            "Customer Jane Doe asked for a change.",
            "Adds the change requested by Jane Doe.",
        ),
        (
            "customer jane doe asked for a change.",
            "Adds the change requested by jane doe.",
        ),
        (
            "The customer jane doe asked for a change.",
            "Adds the change requested by jane doe.",
        ),
    ] {
        let scope = scope_with_private_context(private);
        assert!(
            validate_public_description(description, &scope).is_err(),
            "description unexpectedly passed for {private}"
        );
    }
    let scope = scope_with_private_context("Customer Jane Doe asked for a change.");
    assert!(validate_public_description("Coordinates with Jane.", &scope).is_err());
}

#[test]
fn public_description_allows_technical_prose_after_a_context_word() {
    for (source, description) in [
        (
            "The client API should be documented.",
            "Documents the API should be documented.",
        ),
        (
            "client API requests should be documented.",
            "Documents the API requests endpoint.",
        ),
        (
            "The customer portal should be documented.",
            "Documents the customer portal.",
        ),
    ] {
        let scope = scope_with_private_context(source);
        assert!(
            validate_public_description(description, &scope).is_ok(),
            "ordinary technical prose was rejected for {source}"
        );
    }
}

#[test]
fn contextual_name_detection_does_not_capture_customer_portal_prose() {
    for source in [
        "The customer portal supports offline mode.",
        "The customer support team asked for a change.",
        "The customer contact center reported an outage.",
    ] {
        let fragments = contextual_value_fragments(source);
        assert!(
            !fragments.iter().any(|fragment| {
                fragment.contains("portal supports")
                    || fragment.contains("supportteam")
                    || fragment.contains("contactcenter")
            }),
            "generic technical context was treated as a name: {source}"
        );
    }
}

#[test]
fn public_description_allows_a_summary_without_private_values() {
    let scope =
        scope_with_private_context("Customer: Synthetic Cedar Clinic internal billing migration");
    assert!(
        validate_public_description(
            "Adds bounded retry handling and tests for publication recovery.",
            &scope
        )
        .is_ok()
    );
}

#[test]
fn model_description_rejects_unattributed_contacts_and_identifiers() {
    for description in [
        "Contact synthetic.user@example.invalid for details.",
        "Contact synthetic.user@example.invalid.",
        "Use the callback number 202-555-0199 for confirmation.",
        "Contact at 202 555 0199 for confirmation.",
        "Contact at +1 202 555 0199 for confirmation.",
        "The customer reference is account_4f27a981d6a84b2f.",
    ] {
        assert!(
            validate_model_description(description).is_err(),
            "model output unexpectedly passed: {description}"
        );
    }
}

#[test]
fn model_description_allows_ordinary_technical_identifiers() {
    for description in [
        "Adds retry handling for API clients after a 404 response.",
        "Updates the OAuth flow and tests the v2 adapter.",
    ] {
        assert!(
            validate_model_description(description).is_ok(),
            "ordinary technical output was rejected: {description}"
        );
    }
}
