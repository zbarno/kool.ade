use super::capability_plan_guidance;

#[test]
fn product_plan_requires_stable_registered_readiness_contract() {
    let guidance = capability_plan_guidance(
        &crate::core::opportunity::OpportunityTargetKindV1::KooladeProduct,
    );
    assert!(guidance.contains("## Capability ID"));
    assert!(guidance.contains("compiled active-capability inventory"));
    assert!(guidance.contains("app-owned feature-specific readiness probe"));
    assert!(guidance.contains("self-improvement-lifecycle-v1"));
}

#[test]
fn non_product_plan_does_not_receive_product_capability_requirements() {
    assert!(
        capability_plan_guidance(
            &crate::core::opportunity::OpportunityTargetKindV1::ActiveUserProject
        )
        .is_empty()
    );
}
