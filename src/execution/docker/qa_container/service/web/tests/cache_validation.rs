use super::*;

#[test]
fn web_preview_transfers_only_request_bound_caches_into_offline_service_paths() {
    let (mut request, recipe, image, upload, resources) = fixture();
    request.planning_item_uid = Some(uuid::Uuid::new_v4().hyphenated().to_string());
    let cache = npm_cache_for_arch(&request, "x86_64");
    request.dependency_cache_refs = vec![crate::runtime_qa::QaDependencyCacheRefV1 {
        dependency_request_id: "77777777-7777-4777-8777-777777777777".into(),
        cache_ref: cache.cache_ref.clone(),
    }];
    let spec = QaWebPreviewSpec {
        request: &request,
        browser_image: &image,
        service_image: &image,
        recipe: &recipe,
        artifact_upload: &upload,
        dependency_caches: std::slice::from_ref(&cache),
        preview_port: 4173,
        health_path: "/",
        resources: &resources,
    };

    validate_spec(&spec).unwrap();
    let body = service_body(&spec, &"f".repeat(64)).unwrap();
    let environment = body.env.unwrap();
    assert!(environment.contains(&"NPM_CONFIG_CACHE=/home/executor/.npm".into()));
    assert!(environment.contains(&"NPM_CONFIG_OFFLINE=true".into()));
    let tmpfs = body.host_config.unwrap().tmpfs.unwrap();
    assert!(tmpfs.contains_key("/home/executor/.npm"));
    let allocated_bytes = tmpfs
        .values()
        .map(|mount| {
            mount
                .split(',')
                .find_map(|option| option.strip_prefix("size="))
                .unwrap()
                .parse::<u64>()
                .unwrap()
        })
        .sum::<u64>();
    assert!(allocated_bytes <= resources.temporary_disk_bytes);
}

#[test]
fn web_preview_mounts_the_prepared_pip_wheelhouse_offline() {
    let (mut request, recipe, image, upload, resources) = fixture();
    request.planning_item_uid = Some(uuid::Uuid::new_v4().hyphenated().to_string());
    let cache = pip_cache_for_arch(&request, "amd64");
    request.dependency_cache_refs = vec![crate::runtime_qa::QaDependencyCacheRefV1 {
        dependency_request_id: "77777777-7777-4777-8777-777777777777".into(),
        cache_ref: cache.cache_ref.clone(),
    }];
    let spec = QaWebPreviewSpec {
        request: &request,
        browser_image: &image,
        service_image: &image,
        recipe: &recipe,
        artifact_upload: &upload,
        dependency_caches: std::slice::from_ref(&cache),
        preview_port: 4173,
        health_path: "/",
        resources: &resources,
    };

    validate_spec(&spec).unwrap();
    let body = service_body(&spec, &"f".repeat(64)).unwrap();
    let environment = body.env.unwrap();
    assert!(environment.contains(&"PIP_CONFIG_FILE=/dev/null".into()));
    assert!(environment.contains(&"PIP_NO_INDEX=1".into()));
    assert!(
        environment.contains(&"PIP_FIND_LINKS=/home/executor/.cache/koolade/pip/wheels".into())
    );
    assert!(
        body.host_config
            .unwrap()
            .tmpfs
            .unwrap()
            .contains_key("/home/executor/.cache/koolade/pip")
    );
}

#[test]
fn web_preview_selects_the_request_cache_for_its_pinned_image_architecture() {
    for (image_arch, cache_arch, other_arch) in
        [("amd64", "x86_64", "arm64"), ("arm64", "aarch64", "amd64")]
    {
        let (mut request, recipe, mut image, upload, resources) = fixture();
        image.platform = format!("linux/{image_arch}");
        request.planning_item_uid = Some(uuid::Uuid::new_v4().hyphenated().to_string());
        let cache = npm_cache_for_arch(&request, cache_arch);
        let mut alternate_arch = cache.cache_ref.clone();
        alternate_arch.target_arch = other_arch.into();
        request.dependency_cache_refs = vec![
            crate::runtime_qa::QaDependencyCacheRefV1 {
                dependency_request_id: "77777777-7777-4777-8777-777777777777".into(),
                cache_ref: cache.cache_ref.clone(),
            },
            crate::runtime_qa::QaDependencyCacheRefV1 {
                dependency_request_id: "77777777-7777-4777-8777-777777777777".into(),
                cache_ref: alternate_arch,
            },
        ];
        let spec = QaWebPreviewSpec {
            request: &request,
            browser_image: &image,
            service_image: &image,
            recipe: &recipe,
            artifact_upload: &upload,
            dependency_caches: std::slice::from_ref(&cache),
            preview_port: 4173,
            health_path: "/",
            resources: &resources,
        };

        validate_spec(&spec).unwrap();
    }
}

#[test]
fn web_preview_rejects_cache_archives_not_bound_to_its_immutable_request() {
    let (mut request, recipe, image, upload, resources) = fixture();
    request.planning_item_uid = Some(uuid::Uuid::new_v4().hyphenated().to_string());
    let cache = npm_cache(&request);
    request.dependency_cache_refs = vec![crate::runtime_qa::QaDependencyCacheRefV1 {
        dependency_request_id: "77777777-7777-4777-8777-777777777777".into(),
        cache_ref: cache.cache_ref.clone(),
    }];
    request.dependency_cache_refs[0].cache_ref.target_arch = "arm64".into();
    let spec = QaWebPreviewSpec {
        request: &request,
        browser_image: &image,
        service_image: &image,
        recipe: &recipe,
        artifact_upload: &upload,
        dependency_caches: std::slice::from_ref(&cache),
        preview_port: 4173,
        health_path: "/",
        resources: &resources,
    };

    assert!(validate_spec(&spec).is_err());
}

#[test]
fn dependency_cache_upload_tar_is_readable_by_the_qa_service_user() {
    let (mut request, _, image, _, _) = fixture();
    request.planning_item_uid = Some(uuid::Uuid::new_v4().hyphenated().to_string());
    let cache = npm_cache(&request);
    let archive = dependency_cache::archive_for_runtime_user(&cache, &image.runtime_user).unwrap();
    let mut tar = tar::Archive::new(archive.as_slice());
    let mut entries = tar.entries().unwrap();
    let entry = entries.next().unwrap().unwrap();
    assert_eq!(entry.header().uid().unwrap(), 10001);
    assert_eq!(entry.header().gid().unwrap(), 10001);
    assert_eq!(entry.header().mode().unwrap() & 0o777, 0o600);
    assert_eq!(entry.size(), b"synthetic cached package".len() as u64);
}
