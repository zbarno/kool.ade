use super::*;

struct SplitOpportunityHarness {
    workspace: String,
    product: String,
    user_project_root: std::path::PathBuf,
    product_checkout: std::path::PathBuf,
}

impl AiHarness for SplitOpportunityHarness {
    fn label(&self) -> String {
        "split-opportunity-fixture".into()
    }

    fn check_available(&self) -> Result<String, crate::error::AppError> {
        Ok("fixture ready".into())
    }

    fn execute(
        &self,
        request: &crate::harness::PlanningRequest,
    ) -> Result<HarnessOutcome, crate::error::AppError> {
        let product_scan = request
            .prompt_body
            .contains("PRODUCT-ONLY OPPORTUNITY REVIEW");
        if product_scan {
            assert!(
                !request.prompt_body.contains("PRIVATE-SAMPLE-CUSTOMER-DATA"),
                "the product scan prompt must exclude user project conversation data"
            );
            assert_ne!(request.repo_root, self.user_project_root);
            assert_ne!(request.repo_root, self.product_checkout);
            assert!(
                std::fs::read_dir(&request.repo_root)
                    .unwrap()
                    .next()
                    .is_none(),
                "the product scan cwd must be an empty app-owned directory"
            );
        }
        Ok(HarnessOutcome {
            final_text: if product_scan {
                self.product.clone()
            } else {
                self.workspace.clone()
            },
            envelope: None,
            stderr_tail: String::new(),
        })
    }
}

/// Production boundary: validate and persist a product-target Manager
/// proposal using the public product contract, then reject one that cites
/// private workspace evidence. It also checks the product scan receives no
/// workspace conversation and runs from an empty app-owned directory. Unit schema
/// tests cannot prove prompt, cwd, hashes, confidentiality, repository binding,
/// and durable storage agree. Uses temporary Git repos and local stores; no
/// Docker or network. Setup cost is several Git calls and store writes.
#[test]
fn product_opportunity_isolated_and_persists_public_evidence_only() {
    let (_project_temp, mut project, _) = initialized_repository();
    project.chat.push(crate::domain::chatlog::ChatMessage::new(
        crate::domain::chatlog::ChatRole::User,
        "PRIVATE-SAMPLE-CUSTOMER-DATA: internal account 88421 needs support",
        None,
    ));
    let product_temp = tempfile::tempdir().expect("temporary product checkout");
    git(product_temp.path(), &["init", "-q"]);
    git(
        product_temp.path(),
        &["config", "user.name", "Fixture Product"],
    );
    git(
        product_temp.path(),
        &["config", "user.email", "product@example.invalid"],
    );
    std::fs::write(
        product_temp.path().join("README.md"),
        "public product fixture\n",
    )
    .unwrap();
    git(product_temp.path(), &["add", "README.md"]);
    git(product_temp.path(), &["commit", "-qm", "product fixture"]);
    git(
        product_temp.path(),
        &[
            "remote",
            "add",
            "origin",
            "https://github.com/zbarno/kool.ade",
        ],
    );
    let mut manifest = project.state.repositories.clone();
    manifest
        .repositories
        .push(crate::core::project_repos::Repository {
            id: "product".into(),
            role: "Kool.ad/e product source".into(),
            remote: "https://github.com/zbarno/kool.ade".into(),
            display_name: Some("Kool.ad/e".into()),
        });
    project
        .state
        .planning_store
        .transaction(
            &[(
                crate::artifacts::planning_store::paths::PROJECT_MANIFEST.into(),
                serde_json::to_vec(&manifest).unwrap(),
            )],
            None,
        )
        .unwrap();
    crate::core::project_repos::map_local_checkout(
        &project.state.repo_root,
        "product",
        product_temp.path(),
    )
    .unwrap();
    project.state.repositories = manifest;

    let manager = Manager::start(
        &project,
        &["event-1".into()],
        Box::new(SplitOpportunityHarness {
            workspace: serde_json::json!({
                "summary": "The workspace is healthy.",
                "proposed_actions": [],
                "opportunities": [],
            })
            .to_string(),
            product: serde_json::json!({
                "summary": "A public product improvement is available.",
                "proposed_actions": [],
                "opportunities": [product_proposal_json()],
            })
            .to_string(),
            user_project_root: project.state.repo_root.clone(),
            product_checkout: product_temp.path().to_path_buf(),
        }),
    );
    let deadline = Instant::now() + std::time::Duration::from_secs(10);
    let output = loop {
        if let Some(result) = manager.result() {
            break result.expect("public product proposal should validate");
        }
        assert!(Instant::now() < deadline, "manager harness timed out");
        thread::sleep(std::time::Duration::from_millis(5));
    };

    let product_prompt = super::super::super::catalog::render_product(&project)
        .unwrap()
        .expect("linked product checkout should enable a public scan");
    assert!(
        product_prompt
            .validation
            .public_evidence_ids
            .contains("koolade-product-opportunity-contract")
    );
    assert!(
        product_prompt
            .validation
            .public_evidence_ids
            .contains("koolade-product-capability-inventory")
    );
    assert!(
        product_prompt
            .rendered
            .user
            .contains("target_repository_ref=koolade-product")
    );
    assert!(
        product_prompt
            .rendered
            .user
            .contains("target_repository=https://github.com/zbarno/kool.ade")
    );
    assert!(
        !product_prompt.rendered.user.contains(
            &project
                .state
                .planning_store
                .project_id
                .hyphenated()
                .to_string()
        )
    );
    assert!(
        product_prompt.rendered.user.contains(
            &uuid::Uuid::new_v5(
                &uuid::Uuid::NAMESPACE_URL,
                b"https://github.com/zbarno/kool.ade/public-opportunity-scan",
            )
            .hyphenated()
            .to_string()
        )
    );
    let isolated_root = product_prompt.repo_root.as_ref().unwrap();
    assert_ne!(isolated_root, product_temp.path());
    assert!(std::fs::read_dir(isolated_root).unwrap().next().is_none());
    assert!(
        !product_prompt
            .rendered
            .user
            .contains("PRIVATE-SAMPLE-CUSTOMER-DATA")
    );
    assert!(persist_output(&project, &output).contains("New improvement proposal"));

    let store = crate::core::opportunity::OpportunityStore::new(&project.state.planning_store);
    let saved = store.list().unwrap();
    assert_eq!(saved.len(), 1);
    assert_eq!(
        saved[0].target.kind,
        crate::core::opportunity::OpportunityTargetKindV1::KooladeProduct
    );
    assert!(saved[0].evidence_refs.iter().all(|evidence| {
        evidence.confidentiality == crate::core::opportunity::EvidenceConfidentialityV1::Public
    }));

    let mut private_evidence = product_proposal_json();
    private_evidence["evidence_refs"] = serde_json::json!([
        "koolade-product-capability-inventory",
        "koolade-product-opportunity-contract",
        "generic-capability-inventory"
    ]);
    let manager = Manager::start(
        &project,
        &["event-2".into()],
        Box::new(SplitOpportunityHarness {
            workspace: serde_json::json!({
                "summary": "The workspace is healthy.",
                "proposed_actions": [],
                "opportunities": [],
            })
            .to_string(),
            product: serde_json::json!({
                "summary": "A product proposal cites private data.",
                "proposed_actions": [],
                "opportunities": [private_evidence],
            })
            .to_string(),
            user_project_root: project.state.repo_root.clone(),
            product_checkout: product_temp.path().to_path_buf(),
        }),
    );
    let deadline = Instant::now() + std::time::Duration::from_secs(10);
    let result = loop {
        if let Some(result) = manager.result() {
            break result;
        }
        assert!(Instant::now() < deadline, "manager harness timed out");
        thread::sleep(std::time::Duration::from_millis(5));
    };
    let rejected_private = result.expect("a rejected product scan does not fail workspace Manager");
    assert!(
        rejected_private.opportunities.is_empty(),
        "private workspace evidence must be rejected"
    );
    assert_eq!(store.list().unwrap().len(), 1);
}
