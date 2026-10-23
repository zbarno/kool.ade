use super::super::profile_for_platforms;
use crate::{
    execution::docker::images::RuntimeProfile,
    project_runtime::{AppSurface, ReadinessProbeV1},
    runtime_qa::QaPlatform,
};

#[test]
fn headless_service_uses_the_browser_qa_profile_and_rejects_mixed_platforms() {
    assert_eq!(
        profile_for_platforms(&[QaPlatform::HeadlessService]).unwrap(),
        RuntimeProfile::BrowserQa
    );
    assert!(profile_for_platforms(&[QaPlatform::Web, QaPlatform::HeadlessService]).is_err());
}

#[test]
fn service_readiness_requires_the_probe_for_its_surface_and_approved_port() {
    let probe = ReadinessProbeV1::LocalServiceHealth {
        port: 8080,
        path: "/health".into(),
    };
    assert_eq!(
        super::super::execution::service_readiness_for(
            AppSurface::HeadlessService,
            Some(&probe),
            &[8080],
        )
        .unwrap(),
        (8080, "/health".into())
    );
    assert!(
        super::super::execution::service_readiness_for(AppSurface::Web, Some(&probe), &[8080])
            .is_err()
    );
    assert!(
        super::super::execution::service_readiness_for(
            AppSurface::HeadlessService,
            Some(&probe),
            &[]
        )
        .is_err()
    );
}
