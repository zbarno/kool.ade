use super::*;

#[test]
fn all_role_templates_are_versioned_and_hashes_are_stable() {
    assert_eq!(PromptRole::Planner.id(), "planner/v6");
    assert_eq!(
        PromptRole::PlannerConversationV6.id(),
        "planner_conversation/v6"
    );
    let roles = [
        PromptRole::Manager,
        PromptRole::Planner,
        PromptRole::PlannerConversationV6,
        PromptRole::PlanReviewer,
        PromptRole::PlanReviewerV1,
        PromptRole::Decomposer,
        PromptRole::Implementer,
        PromptRole::CodeReviewer,
        PromptRole::CodeReviewerV1,
        PromptRole::RuntimeQaReviewer,
        PromptRole::RuntimeQaReviewerV2,
        PromptRole::RuntimeQaReviewerV3,
        PromptRole::RuntimeQaScenarioSynthesizer,
        PromptRole::SpecReconciler,
        PromptRole::RepairWorker,
        PromptRole::PrPreparer,
        PromptRole::BootstrapConversation,
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
            "06e0a9fdf0272b1c66b61c7f26642e71312735c15e840080efbc36f12a5432dc",
            "30cd1a4c17f1eaaca94a3274b52f251bc757d3a902bf0601f48813b49e9315ac",
        ),
        (
            PromptRole::Planner,
            "7590c196cab433c3d61fa7750285071367b4b3d5baadc5d9740321b19d3e8a4a",
            "a314b43f01133cc3f580d68776795502e14e6a099992c4c1a38abab2a14f584d",
        ),
        (
            PromptRole::PlannerConversationV6,
            "700d7c1662da977329dffebca1c1e3d29f08af6938f5194ca47a76d659cfa338",
            "98e0693846529ebc50343e77dccccd2b8aca76421272101f657c954dfe453df8",
        ),
        (
            PromptRole::PlanReviewer,
            "d72dfc0ee5938bb7725b8aa0e0ac56bcd9eee2533563f022f6a0023290a222c4",
            "528596028f461c30c4f0d8f985f03e03ae62944a55669824bb6b02c36da2b40b",
        ),
        (
            PromptRole::PlanReviewerV1,
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
            "75b00ebee8fbc8197717d054f7f7afbc3f8566fe1a4c5124a129b6d040249d65",
            "1214d63c183650c99122395cb2e005cc8a8327f9da04578ce72cbbea9bddb687",
        ),
        (
            PromptRole::CodeReviewerV1,
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
            PromptRole::RuntimeQaReviewerV3,
            "92ab9a496948007cbcc4014145cc24263ca6720637f5ba33fe8d54ef61c739aa",
            "d119e7f19d5d92ebd056f2fbe30d86361bb55febccec25db5524660c9f89fb81",
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
        (
            PromptRole::BootstrapConversation,
            "500549a8692ac5bede8269f59eec634027891e056cd32d22fe9e3dd89fd76e6e",
            "270ae885b66059807edc4611117facceca34f9ad3287aecabf5ed93b9b484b2a",
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
        template_hash(PromptRole::PlannerConversationV6.policy()),
        "aed16e82f1d61b2a05be2f80b59cd60fd6aa31bd3ad0cae3f7f8991a196fc4a8",
        "planner conversation policy changed without a version bump"
    );
    let generic_policy_hash = "7d883b0198159f09d1f87d2f3ca4e3c09683d06617eb6082aa22e5147b4e871a";
    for role in [
        PromptRole::Planner,
        PromptRole::PlanReviewer,
        PromptRole::Decomposer,
        PromptRole::Implementer,
        PromptRole::CodeReviewer,
        PromptRole::CodeReviewerV1,
        PromptRole::RuntimeQaReviewer,
        PromptRole::RuntimeQaReviewerV2,
        PromptRole::RuntimeQaReviewerV3,
        PromptRole::SpecReconciler,
        PromptRole::RepairWorker,
        PromptRole::PrPreparer,
        PromptRole::BootstrapConversation,
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

#[test]
fn runtime_qa_v3_requires_visual_screenshot_inspection() {
    let system = PromptRole::RuntimeQaReviewerV3.system();
    let user = PromptRole::RuntimeQaReviewerV3.user();
    assert!(system.contains("koolade_runtime_qa_screenshot"));
    assert!(system.contains("inspect the actual pixels"));
    assert!(user.contains("inspect returned pixels before citing them"));
    assert!(system.contains("You do not launch or control browser or desktop drivers"));
}
