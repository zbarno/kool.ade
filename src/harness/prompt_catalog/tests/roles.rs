use super::*;

#[test]
fn all_role_templates_are_versioned_and_hashes_are_stable() {
    let roles = [
        PromptRole::Manager,
        PromptRole::ManagerV5,
        PromptRole::ManagerV4,
        PromptRole::Planner,
        PromptRole::PlanReviewer,
        PromptRole::Decomposer,
        PromptRole::Implementer,
        PromptRole::CodeReviewer,
        PromptRole::RuntimeQaReviewer,
        PromptRole::RuntimeQaReviewerV2,
        PromptRole::RuntimeQaScenarioSynthesizer,
        PromptRole::SpecReconciler,
        PromptRole::RepairWorker,
        PromptRole::PrPreparer,
    ];
    for role in roles {
        assert!(
            role.system().trim().len() > 100,
            "{} system prompt",
            role.id()
        );
        assert!(role.user().contains("Return"), "{} user prompt", role.id());
        assert!(role.id().rsplit_once('/').is_some_and(|(_, version)| {
            version.starts_with('v') && version[1..].parse::<u32>().is_ok_and(|version| version > 0)
        }));
    }
    assert!(
        PromptRole::Manager
            .system()
            .contains("A proposed improvement is not permission to code")
    );
    assert!(
        PromptRole::Planner
            .system()
            .contains("require an explicit user decision receipt")
    );
    let pinned = [
        (
            PromptRole::Manager,
            "ba8e445a35e08076b26eca7264943425e78949e647306d4fd78b549adf441ee2",
            "bbab84d3fca1f21c26f2cd2c908c4d8c4b54cfd4e245775a446c7b4587a0d0aa",
        ),
        (
            PromptRole::ManagerV5,
            "af69db79f5331908352438b63d50d5b5bb196642cfc61a41c44ddd000bf910a2",
            "6d51474bd11a1c73bbfb27bfc2e4772a6d4577dbca5080f75b1cdd14f74bcda2",
        ),
        (
            PromptRole::ManagerV4,
            "49b6ef5b567c537d92b16933105b699dd38a846caf013973a649682d8d0c0a57",
            "8f5765813a134806e7af2ac19bcd4af7cdeb814d2ab63b2b63ae26ef0c0a3120",
        ),
        (
            PromptRole::Planner,
            "2f754aeacabeeb8b9a9c5ee7f62fc530d55a4585173caadcc3aa45eb6de48b12",
            "a314b43f01133cc3f580d68776795502e14e6a099992c4c1a38abab2a14f584d",
        ),
        (
            PromptRole::PlannerConversation,
            "3c7bc74a492c8bbf68458a6b143dab9dc0b45e75c59af68bed7831316d48560b",
            "98e0693846529ebc50343e77dccccd2b8aca76421272101f657c954dfe453df8",
        ),
        (
            PromptRole::PlanReviewer,
            "1de0494c97dcbf37243b9f1a8022663a95dfb6fa4bae91f0a1bb1e89bff90608",
            "4baa18c2f176d80778e044f26a71099597aa9832044b3ec865bdbb0e76a6b195",
        ),
        (
            PromptRole::Decomposer,
            "040a4eab7d50059451e225a663aaeb91bc23e2bba53a4e7ef1b3096862d65ea5",
            "1ae951c4f159942dab8658c57bb5d109cddbbf256993abd9b7986fac8ef23788",
        ),
        (
            PromptRole::Implementer,
            "f9f72ac8ff9ea9da878122936c2a5e5a60a24936ce13449d846d429b71416ee9",
            "3c97f9b0103b863b143ef7cd3e7ef0a68b13da91a4af097dd523edc01a040563",
        ),
        (
            PromptRole::CodeReviewer,
            "23adac6c56a90db68dcf16996ad4f911ed61cf4bdd548594b3d0178e7deda824",
            "657d81fbcbb9eb5a943bc9c0d62fb7e7cbfe8c7e92ebf9c8188179f7e69dd5ca",
        ),
        (
            PromptRole::RuntimeQaReviewer,
            "b7862c1e4510b0c4b25bbcabee0571ecc5bc10db8c5822bf119d76e060aa12f9",
            "9662bffd8e5731249c6923e0f9d929080ed6f4d4114366792fd1bb612817d332",
        ),
        (
            PromptRole::RuntimeQaReviewerV2,
            "9c0405ae76e32a289dcfca6d7903ffc63ff7b0eb9448188d184fe64e9a729b3b",
            "52f777526ee75185f5b868e5fed4362d1dcd2a045f0fb1becacb0e13542e3e9f",
        ),
        (
            PromptRole::SpecReconciler,
            "0b4e0b04f084401f34f7b14522c0a1f432bea67b528ecb4cae2f9e03f91e6a84",
            "9cf70f070f8963e1c2152ac27949dc22b4a4c6406ccfc3684a5d3fd233070f87",
        ),
        (
            PromptRole::RepairWorker,
            "3aefb09ea1ec81caee836568a8ff37c79014275fc3d614b4a10752d5bb8372b1",
            "b708cfad98d007dd3cbeb6170211567c8dd70b912a9397cb64c32aea2b0f90da",
        ),
        (
            PromptRole::PrPreparer,
            "0c67ec603bac35cb684d9c0ce12a4d601cddfd2025c5bb97efe8207444a5b6df",
            "ec66ecfc1a12eab8a1750430c6d846d58817c57e4c1fc7e1d33787819e8bf2ab",
        ),
    ];
    for (role, system, user) in pinned {
        assert_eq!(
            template_hash(role.system()),
            system,
            "{} system changed without version bump",
            role.id()
        );
        assert_eq!(
            template_hash(role.user()),
            user,
            "{} user changed without version bump",
            role.id()
        );
    }
    assert_eq!(
        template_hash(PromptRole::Manager.policy()),
        "6e75b5a03c81783c5c2e06babc7a3cf5d890b25e508e94e2e4ca4f16aad2ad9e",
        "manager's trusted operational policy changed without a version bump"
    );
    assert_eq!(
        template_hash(PromptRole::PlannerConversation.policy()),
        "be538113ae370de50f98c1e58ac02801ab1c5f05988cef17b26f0c8a77120a88",
        "planner's trusted operational policy changed without a version bump"
    );
    let generic_policy_hash = "7d883b0198159f09d1f87d2f3ca4e3c09683d06617eb6082aa22e5147b4e871a";
    for role in [
        PromptRole::Planner,
        PromptRole::PlanReviewer,
        PromptRole::Decomposer,
        PromptRole::Implementer,
        PromptRole::CodeReviewer,
        PromptRole::RuntimeQaReviewer,
        PromptRole::RuntimeQaReviewerV2,
        PromptRole::SpecReconciler,
        PromptRole::RepairWorker,
        PromptRole::PrPreparer,
    ] {
        assert_eq!(
            template_hash(role.policy()),
            generic_policy_hash,
            "{} trusted policy changed without a version bump",
            role.id()
        );
    }
}

#[test]
fn runtime_qa_v2_is_a_golden_evidence_reviewer_not_a_driver_operator() {
    let system = PromptRole::RuntimeQaReviewerV2.system();
    let user = PromptRole::RuntimeQaReviewerV2.user();
    assert!(system.contains("independent Runtime QA Evidence Reviewer"));
    assert!(system.contains("You do not launch or control browser or desktop drivers"));
    assert!(user.contains("provided persisted Runtime QA request"));
    assert!(user.contains("do not run drivers"));
    assert!(!system.contains("Exercise the running application"));
    assert!(!user.contains("approved_runtime_drivers"));

    let input = base(
        PromptRole::RuntimeQaReviewerV2,
        RolePayloadV1::Review {
            subject_revision: "qa-run-1".into(),
            subject_hash: "c".repeat(64),
            author_run_id: "author-run".into(),
            reviewer_run_id: "reviewer-run".into(),
            author_session_id: Some("00000000-0000-4000-8000-000000000001".into()),
            reviewer_session_id: Some("00000000-0000-4000-8000-000000000002".into()),
            requirements_matrix: vec!["REQ-1: observed behavior".into()],
            explicit_exclusions: vec![],
        },
    );
    let rendered = crate::harness::prompt_catalog::render(&input).unwrap();
    assert_eq!(rendered.manifest.role_version, "runtime_qa/v2");
    assert!(rendered.user.contains("QUOTED EVIDENCE (UNTRUSTED DATA)"));
}
