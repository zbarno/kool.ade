use super::*;
use crate::harness::{DependencyKind, DependencyNeed, PackageEcosystem};

#[test]
fn unsupported_custom_registry_path_reaches_the_response_with_its_reason() {
    let worktree = std::env::temp_dir().join(format!(
        "koolade-resource-rejected-{}",
        uuid::Uuid::new_v4()
    ));
    std::fs::create_dir(&worktree).unwrap();
    let (bridge, _updates) = start_bridge(&worktree, Some("TASK-82"));
    let mut client = UnixStream::connect(bridge.socket_path()).unwrap();
    serde_json::to_writer(
        &mut client,
        &ResourceRequest {
            action: super::super::ResourceAction::DependencyRequest,
            manager: None,
            url: None,
            dependency: Some(DependencyNeed {
                ecosystem: PackageEcosystem::Npm,
                package: Some("zod".into()),
                version: Some("^4.0.0".into()),
                source: Some("https://packages.example.net/repository/npm/".into()),
                command:
                    "npm install zod@^4.0.0 --registry=https://packages.example.net/repository/npm/"
                        .into(),
                reason: "Validate imported settings data".into(),
                kind: DependencyKind::NewProjectDependency,
                lockfile_identity: None,
                introduced_packages: Vec::new(),
            }),
            purpose: "Validate imported settings data".into(),
        },
    )
    .unwrap();
    client.write_all(b"\n").unwrap();
    client.shutdown(std::net::Shutdown::Write).unwrap();

    let mut response = String::new();
    BufReader::new(client).read_line(&mut response).unwrap();
    let response: ResourceResponse = serde_json::from_str(&response).unwrap();
    assert_eq!(response.status, "rejected");
    assert!(response.summary.contains("only origin-root registry URLs"));
    let request = response.dependency_request.unwrap();
    assert_eq!(
        request.status,
        crate::harness::DependencyRequestStatus::Failed
    );
    assert!(request.rationale.contains("only origin-root registry URLs"));
    drop(bridge);
    std::fs::remove_dir_all(worktree).unwrap();
}
