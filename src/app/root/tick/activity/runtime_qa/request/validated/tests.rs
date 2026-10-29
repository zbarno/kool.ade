use super::*;

#[test]
fn reviewed_qa_requests_keep_every_architecture_specific_dependency_cache() {
    let cache_ref =
        |target_arch: &str, hash: char| crate::execution::contract::DependencyCacheRefV1 {
            ecosystem: crate::execution::contract::DependencyCacheEcosystemV1::Cargo,
            task_id: "11111111-1111-4111-8111-111111111111".into(),
            project_id: "22222222-2222-4222-8222-222222222222".into(),
            resolved_integrity: "sha256:lockfile".into(),
            target_arch: target_arch.into(),
            target_id: "local".into(),
            manifest_sha256: hash.to_string().repeat(64),
            size_bytes: 1,
        };
    let alternatives = vec![
        (
            "33333333-3333-4333-8333-333333333333".into(),
            cache_ref("amd64", 'a'),
        ),
        (
            "33333333-3333-4333-8333-333333333333".into(),
            cache_ref("arm64", 'b'),
        ),
    ];

    let request_refs = qa_dependency_cache_refs(&alternatives);

    assert_eq!(request_refs.len(), 2);
    assert_eq!(
        request_refs[0].dependency_request_id,
        request_refs[1].dependency_request_id
    );
    assert_ne!(
        request_refs[0].cache_ref.target_arch,
        request_refs[1].cache_ref.target_arch
    );
}
