use super::*;

fn cache_ref(task_id: &str, target_arch: &str, hash: char) -> QaDependencyCacheRefV1 {
    QaDependencyCacheRefV1 {
        dependency_request_id: "77777777-7777-4777-8777-777777777777".into(),
        cache_ref: crate::execution::contract::DependencyCacheRefV1 {
            ecosystem: crate::execution::contract::DependencyCacheEcosystemV1::Npm,
            task_id: task_id.into(),
            project_id: "88888888-8888-4888-8888-888888888888".into(),
            resolved_integrity: "sha256:lockfile".into(),
            target_arch: target_arch.into(),
            target_id: "local".into(),
            manifest_sha256: hash.to_string().repeat(64),
            size_bytes: 1,
        },
    }
}

#[test]
fn runtime_qa_request_can_bind_architecture_specific_cache_alternatives() {
    let mut request = request();
    request.planning_item_uid = Some(request.parent_task_uid.clone());
    request.dependency_cache_refs = vec![
        cache_ref(&request.parent_task_uid, "amd64", 'a'),
        cache_ref(&request.parent_task_uid, "arm64", 'b'),
    ];

    assert!(request.validate().is_ok());

    request.dependency_cache_refs[1].cache_ref.target_arch = "x86_64".into();
    assert!(request.validate().is_err());
}

#[test]
fn runtime_qa_request_accepts_namespaced_dependency_request_ids() {
    let mut request = request();
    request.planning_item_uid = Some(request.parent_task_uid.clone());
    let mut dependency = cache_ref(&request.parent_task_uid, "amd64", 'a');
    dependency.dependency_request_id = format!("dependency-{}", uuid::Uuid::new_v4());
    request.dependency_cache_refs = vec![dependency];

    assert!(request.validate().is_ok());

    request.dependency_cache_refs[0].dependency_request_id = "dependency-invalid".into();
    assert!(request.validate().is_err());
}
