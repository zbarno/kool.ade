use super::*;

#[test]
fn all_role_templates_are_versioned_and_hashes_are_stable() {
    let roles = [
        PromptRole::Manager,
        PromptRole::Planner,
        PromptRole::PlanReviewer,
        PromptRole::Decomposer,
        PromptRole::Implementer,
        PromptRole::CodeReviewer,
        PromptRole::RuntimeQaReviewer,
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
            "49b6ef5b567c537d92b16933105b699dd38a846caf013973a649682d8d0c0a57",
            "8f5765813a134806e7af2ac19bcd4af7cdeb814d2ab63b2b63ae26ef0c0a3120",
        ),
        (
            PromptRole::Planner,
            "41709cb904f48b8743acf8434ec65b000835b8e2698f5512ba2923a7e468edfa",
            "88d1cfcf1d08b2d3a28185d64422887eb6022f55d8db1f7b9984da9e09d31ab5",
        ),
        (
            PromptRole::PlanReviewer,
            "d656967124b8db6e1dfe20a51e85254982c6f075889b75ef0372b37953f8e607",
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
    let generic_policy_hash = "7d883b0198159f09d1f87d2f3ca4e3c09683d06617eb6082aa22e5147b4e871a";
    for role in [
        PromptRole::Planner,
        PromptRole::PlanReviewer,
        PromptRole::Decomposer,
        PromptRole::Implementer,
        PromptRole::CodeReviewer,
        PromptRole::RuntimeQaReviewer,
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
