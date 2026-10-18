use super::*;

#[derive(serde::Deserialize)]
struct OpportunityEvalFixture {
    id: String,
    evidence: String,
    expected: String,
    origin: Option<String>,
    kind: Option<String>,
    #[serde(default)]
    event_evidence: Vec<String>,
    attack: Option<String>,
    output: serde_json::Value,
}

#[test]
fn versioned_opportunity_golden_outputs_match_policy_contract_and_validate() {
    let fixtures: Vec<OpportunityEvalFixture> = serde_json::from_str(include_str!(
        "../../../../../prompts/evals/v2/opportunities.json"
    ))
    .unwrap();
    let ids = fixtures
        .iter()
        .map(|fixture| fixture.id.as_str())
        .collect::<std::collections::HashSet<_>>();
    assert_eq!(ids.len(), fixtures.len(), "fixture IDs must be unique");
    for required in [
        "generic-capability-existing",
        "unsupported-dependency",
        "novel-feature",
        "recurring-bug",
        "security-finding",
        "duplicate-declined",
        "dismissed-idea",
        "malicious-approved-field",
        "public-confidentiality",
        "wrong-target-repository",
        "no-background-hallucination",
    ] {
        assert!(
            ids.contains(required),
            "missing scenario fixture {required}"
        );
    }

    let manager_system = include_str!("../../../../../prompts/v5/manager.system.md");
    let manager_user = include_str!("../../../../../prompts/v5/manager.user.md");
    assert!(manager_system.contains("Only an explicit user choice may start planning"));
    assert!(manager_system.contains("Before proposing a missing feature, verify existing general-purpose capabilities and workarounds"));
    assert!(manager_system.contains("A generic capability that solves the task is not a blocker"));
    assert!(manager_system.contains("koolade-product-opportunity-contract"));
    assert!(manager_system.contains("do not create a duplicate unsolicited opportunity card"));
    assert!(manager_user.contains("if it is absent or invalid, say capability state is unknown"));
    assert!(manager_user.contains("Never claim an unverified capability is missing"));
    assert!(manager_user.contains("do not cite workspace-private user evidence"));
    assert!(manager_user.contains("do not emit a duplicate opportunity"));

    for fixture in fixtures {
        let mut validation = context();
        if fixture.attack.as_deref() == Some("private_cross_repository_evidence") {
            validation.allowed_repositories.insert(
                "koolade".into(),
                "https://github.com/zbarno/kool.ade".into(),
            );
            validation.koolade_product_repository_ref = Some("koolade".into());
            validation.public_evidence_ids = ["generic-capability-inventory".into()].into();
        }
        let opportunities = fixture.output["opportunities"]
            .as_array()
            .expect("golden manager output has an opportunities array");
        match fixture.expected.as_str() {
            "no_proposal" => assert!(
                opportunities.is_empty(),
                "{} golden output must not propose work",
                fixture.id
            ),
            "valid_proposal" => {
                assert_eq!(
                    opportunities.len(),
                    1,
                    "{} golden proposal count",
                    fixture.id
                );
                let candidate = &opportunities[0];
                assert_eq!(candidate["origin"], fixture.origin.as_deref().unwrap());
                assert_eq!(candidate["kind"], fixture.kind.as_deref().unwrap());
                assert_eq!(candidate["problem_or_opportunity"], fixture.evidence);
                for reference in &fixture.event_evidence {
                    assert!(
                        candidate["evidence_refs"]
                            .as_array()
                            .unwrap()
                            .iter()
                            .any(|item| item == reference),
                        "{} must cite {reference}",
                        fixture.id
                    );
                    validation.evidence_ids.insert(reference.clone());
                }
            }
            "reject" => assert!(fixture.attack.is_some(), "{} attack kind", fixture.id),
            other => panic!("unknown expected result {other}"),
        }
        let result = validate_output_with_context(
            PromptRole::Manager,
            &fixture.output.to_string(),
            &validation,
        );
        match fixture.expected.as_str() {
            "reject" => assert!(result.is_err(), "{} must be rejected", fixture.id),
            "valid_proposal" | "no_proposal" => {
                assert!(result.is_ok(), "{} must validate: {result:?}", fixture.id)
            }
            _ => unreachable!(),
        }
    }
}
