use super::initialized_repository;

/// Production boundary: fresh operator-local provider discovery is attached to
/// the workspace Manager lookup but remains outside the public product scan.
/// A catalog unit test cannot prove route selection, prompt wiring, or evidence
/// confidentiality. Uses only a temporary repository/settings file; low setup
/// cost and no provider process, network, Docker, or desktop session.
#[test]
fn manager_provider_unavailability_is_scoped_and_never_leaks_to_product_scan() {
    const COMMIT: &str = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";
    let _identity = crate::build_identity::override_for_test("0.1.0", COMMIT);
    let (repo_temp, mut project, _) = initialized_repository();
    project.chat.push(crate::domain::chatlog::ChatMessage::new(
        crate::domain::chatlog::ChatRole::User,
        "Is provider routing and role isolation ready?",
        None,
    ));
    project.state.repositories.repositories[0].remote = "https://github.com/zbarno/kool.ade".into();

    let settings_path = repo_temp.path().join("harness-settings.json");
    crate::persistence::harness_settings::with_settings_path(settings_path, || {
        let now = crate::persistence::harness_settings::current_unix_ms();
        let mut settings = crate::persistence::harness_settings::HarnessSettings {
            work_routes: std::collections::BTreeMap::from([(
                crate::persistence::harness_settings::MANAGER.into(),
                crate::persistence::harness_settings::WorkRoute {
                    harness: "pi".into(),
                    model: Some("synthetic-model".into()),
                },
            )]),
            discovered: std::collections::BTreeMap::from([(
                "pi".into(),
                crate::persistence::harness_settings::DetectedHarness {
                    status: "unavailable".into(),
                    version: None,
                    executable: None,
                    diagnostic: Some("SYNTHETIC_PRIVATE_PROVIDER_DIAGNOSTIC".into()),
                    ready: false,
                    models: Vec::new(),
                    default_model: None,
                    model_catalog_fresh: false,
                    configuration_required: true,
                    implementation_available: false,
                    capability_snapshot: None,
                    checked_at_unix_ms: now,
                },
            )]),
            ..Default::default()
        };
        crate::persistence::harness_settings::save(&settings).unwrap();

        let workspace_prompt = super::super::super::catalog::render(&project, &[]).unwrap();
        let lookup = workspace_prompt
            .validation
            .evidence_content
            .get("product-capability-lookup")
            .unwrap();
        assert!(lookup.contains("provider-routing-and-role-isolation"));
        assert!(lookup.contains("ImplementedButNotReady"));
        assert!(!lookup.contains("SYNTHETIC_PRIVATE_PROVIDER_DIAGNOSTIC"));
        assert!(
            workspace_prompt
                .evidence_confidentialities
                .get("product-capability-lookup")
                .is_some_and(|value| *value
                    == crate::harness::prompt_catalog::EvidenceConfidentiality::WorkspacePrivate)
        );

        settings.discovered.clear();
        crate::persistence::harness_settings::save(&settings).unwrap();
        let missing_prompt = super::super::super::catalog::render(&project, &[]).unwrap();
        assert!(
            missing_prompt.validation.evidence_content["product-capability-lookup"]
                .contains("state=Unknown")
        );

        settings.discovered.insert(
            "pi".into(),
            ready_provider(
                now.saturating_sub(
                    crate::persistence::harness_settings::DISCOVERY_FRESHNESS_MS + 1,
                ),
            ),
        );
        crate::persistence::harness_settings::save(&settings).unwrap();
        let stale_prompt = super::super::super::catalog::render(&project, &[]).unwrap();
        assert!(
            stale_prompt.validation.evidence_content["product-capability-lookup"]
                .contains("state=Unknown")
        );

        settings.discovered.insert("pi".into(), ready_provider(now));
        crate::persistence::harness_settings::save(&settings).unwrap();
        project.state.harness_routes.routes.insert(
            crate::persistence::harness_settings::MANAGER.into(),
            crate::persistence::harness_settings::WorkRoute {
                harness: "pi".into(),
                model: Some("unverified-project-model".into()),
            },
        );
        let overridden_prompt = super::super::super::catalog::render(&project, &[]).unwrap();
        assert!(
            overridden_prompt.validation.evidence_content["product-capability-lookup"]
                .contains("ImplementedButNotReady")
        );
        project
            .state
            .harness_routes
            .routes
            .get_mut(crate::persistence::harness_settings::MANAGER)
            .unwrap()
            .model = Some("synthetic-model".into());
        let ready_prompt = super::super::super::catalog::render(&project, &[]).unwrap();
        let ready_lookup = &ready_prompt.validation.evidence_content["product-capability-lookup"];
        assert!(
            ready_lookup.contains("state=VerifiedReady"),
            "expected fresh route evidence, got: {ready_lookup}"
        );

        let mut unknown = ready_provider(now);
        unknown.capability_snapshot.as_mut().unwrap().authentication =
            crate::execution::contract::ReadinessState::Unknown;
        settings.discovered.insert("pi".into(), unknown);
        crate::persistence::harness_settings::save(&settings).unwrap();
        let unknown_prompt = super::super::super::catalog::render(&project, &[]).unwrap();
        assert!(
            unknown_prompt.validation.evidence_content["product-capability-lookup"]
                .contains("state=Unknown")
        );

        let product_prompt = super::super::super::catalog::render_product(&project)
            .unwrap()
            .unwrap();
        assert!(
            !product_prompt
                .rendered
                .user
                .contains("ImplementedButNotReady")
        );
        assert!(
            !product_prompt
                .rendered
                .user
                .contains("SYNTHETIC_PRIVATE_PROVIDER_DIAGNOSTIC")
        );
        assert!(
            product_prompt
                .validation
                .public_evidence_ids
                .contains("koolade-product-capability-inventory")
        );
    });
}

fn ready_provider(
    checked_at_unix_ms: u64,
) -> crate::persistence::harness_settings::DetectedHarness {
    use crate::execution::contract::{
        HarnessCapabilitySnapshotV1, HarnessRole, ModeReadinessV1, ReadinessState, RoleReadinessV1,
    };
    let provider_id = "pi";
    let snapshot = HarnessCapabilitySnapshotV1 {
        schema_version: 1,
        provider_id: provider_id.into(),
        authentication_mode: crate::execution::contract::current_authentication_mode(provider_id),
        manager_profile_id: crate::execution::contract::profile_evidence_id_for_current_snapshot(
            provider_id,
            Some("1.0.0"),
            crate::harness::ExecutionProfile::Manager,
        ),
        worker_profile_id: crate::execution::contract::profile_evidence_id_for_current_snapshot(
            provider_id,
            Some("1.0.0"),
            crate::harness::ExecutionProfile::Worker,
        ),
        probe_id: uuid::Uuid::new_v4().hyphenated().to_string(),
        probe_cancellation_generation: 1,
        probe_sequence: 1,
        cli_binary: ReadinessState::Ready,
        cli_version: Some("1.0.0".into()),
        authentication: ReadinessState::Ready,
        model_catalog: ReadinessState::Ready,
        scriptable_invocation: ReadinessState::Ready,
        docker_mcp_bridge: ReadinessState::Ready,
        execution_target: ReadinessState::Ready,
        failure: None,
        modes: crate::harness::ExecutionMode::ALL
            .into_iter()
            .map(|mode| ModeReadinessV1 {
                mode,
                available: ReadinessState::Ready,
                failure: None,
            })
            .collect(),
        roles: HarnessRole::ALL
            .into_iter()
            .map(|role| RoleReadinessV1 {
                role,
                available: ReadinessState::Ready,
                failure: None,
            })
            .collect(),
    };
    crate::persistence::harness_settings::DetectedHarness {
        status: "synthetic ready provider".into(),
        version: Some("1.0.0".into()),
        executable: Some("/synthetic/bin/pi".into()),
        diagnostic: None,
        ready: true,
        models: vec!["synthetic-model".into()],
        default_model: Some("synthetic-model".into()),
        model_catalog_fresh: true,
        configuration_required: false,
        implementation_available: true,
        capability_snapshot: Some(snapshot),
        checked_at_unix_ms,
    }
}
