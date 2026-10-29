use super::*;

#[test]
fn desktop_driver_accepts_equivalent_cache_architecture_aliases() {
    use crate::execution::contract::DependencyCacheEcosystemV1;

    let root = tempfile::tempdir().unwrap();
    std::fs::write(root.path().join("package.tgz"), b"approved offline package").unwrap();
    for (image_arch, cache_arch) in [("amd64", "x86_64"), ("arm64", "aarch64")] {
        let mut request = super::request();
        request.relevant_platforms = vec![crate::runtime_qa::QaPlatform::LinuxDesktop];
        let planning_uid = uuid::Uuid::new_v4().hyphenated().to_string();
        request.planning_item_uid = Some(planning_uid.clone());
        let cache = crate::harness::resource_bridge::CacheSnapshot::capture(
            root.path(),
            &planning_uid,
            DependencyCacheEcosystemV1::Npm,
            "88888888-8888-4888-8888-888888888888",
            "sha256:synthetic-lockfile",
            cache_arch,
            "local",
        )
        .unwrap();
        request.dependency_cache_refs = vec![crate::runtime_qa::QaDependencyCacheRefV1 {
            dependency_request_id: uuid::Uuid::new_v4().hyphenated().to_string(),
            cache_ref: cache.cache_ref.clone(),
        }];
        let mut image = super::selected_qa_image(RuntimeProfile::DesktopQa);
        image.platform = format!("linux/{image_arch}");

        assert!(
            super::driver_cache::validate_for_driver(
                "desktop",
                &request,
                &[cache],
                &image,
                &super::resources(),
            )
            .is_ok()
        );
    }
}
