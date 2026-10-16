use super::*;
use sha2::{Digest, Sha256};

const TASK_ID: &str = "00000000-0000-4000-8000-000000000137";
const PROJECT_ID: &str = "00000000-0000-4000-8000-000000000138";

fn request() -> SystemImageExtensionRequest {
    let base = ImageLock::bundled()
        .unwrap()
        .select(RuntimeProfile::General, "x86_64")
        .unwrap();
    SystemImageExtensionRequest {
        task_id: TASK_ID.into(),
        project_id: PROJECT_ID.into(),
        distribution: LinuxDistribution::DebianBookworm,
        base_profile: RuntimeProfile::General,
        base_image_digest: base.digest,
        target_arch: "x86_64".into(),
        packages: vec!["ca-certificates".into(), "pkg-config".into()],
        trust: PackageTrust::ApprovedDistributionRepository,
        purpose: "Provide system headers needed by this task's native package build".into(),
    }
}

fn closure(request: &SystemImageExtensionRequest) -> Vec<builder::PackageArtifactIdentity> {
    request
        .packages
        .iter()
        .map(|name| builder::PackageArtifactIdentity {
            name: name.clone(),
            version: "1.0-1".into(),
            architecture: "amd64".into(),
            source: format!(
                "https://deb.debian.org/debian/pool/main/{}/{}/{}_1.0-1_amd64.deb",
                &name[..1],
                name,
                name
            ),
            sha256: format!("{:x}", Sha256::digest(name.as_bytes())),
        })
        .collect()
}

#[test]
fn authorized_request_uses_locked_base_and_canonical_request_identity() {
    let proposed = request();
    let plan = proposed
        .clone()
        .authorize(
            DependencyDecision::UserAuthorizeForTask,
            TASK_ID,
            closure(&proposed),
        )
        .unwrap();
    assert!(plan.base_reference.ends_with(&plan.base_digest));
    assert_eq!(plan.target_platform, "linux/amd64");
    assert_eq!(plan.requested_packages, ["ca-certificates", "pkg-config"]);
    assert_eq!(plan.request_identity.len(), 71);
}

#[test]
fn manager_recommendation_cannot_authorize_system_image_extension() {
    let proposed = request();
    let error = proposed
        .clone()
        .clone()
        .clone()
        .authorize(
            DependencyDecision::AuthorizeForTask,
            TASK_ID,
            closure(&proposed),
        )
        .unwrap_err();
    assert!(
        error
            .to_string()
            .contains("explicit task-scoped user authorization")
    );
}

#[test]
fn denied_user_request_cannot_derive_a_system_image() {
    let proposed = request();
    let error = proposed
        .clone()
        .authorize(DependencyDecision::Reject, TASK_ID, closure(&proposed))
        .unwrap_err();
    assert!(
        error
            .to_string()
            .contains("explicit task-scoped user authorization")
    );
}

#[test]
fn extension_rejects_other_task_third_party_source_and_command_injection() {
    let error = request()
        .authorize(
            DependencyDecision::UserAuthorizeForTask,
            "00000000-0000-4000-8000-000000000139",
            closure(&request()),
        )
        .unwrap_err();
    assert!(
        error
            .to_string()
            .contains("outside the selected task/project")
    );

    let mut third_party = request();
    third_party.trust = PackageTrust::ThirdPartyRepository;
    assert!(
        third_party
            .clone()
            .authorize(
                DependencyDecision::UserAuthorizeForTask,
                TASK_ID,
                closure(&third_party),
            )
            .unwrap_err()
            .to_string()
            .contains("only the approved Debian Bookworm archive is supported")
    );

    let mut injected = request();
    injected.packages[0] = "ca-certificates;touch".into();
    injected.packages.sort();
    assert!(
        injected
            .clone()
            .authorize(
                DependencyDecision::UserAuthorizeForTask,
                TASK_ID,
                closure(&injected),
            )
            .unwrap_err()
            .to_string()
            .contains("package names are invalid or not canonical")
    );
}

#[test]
fn extension_rejects_unapproved_base_digest_and_unsorted_duplicate_packages() {
    let mut wrong_base = request();
    wrong_base.base_image_digest = format!("sha256:{}", "a".repeat(64));
    assert!(
        wrong_base
            .clone()
            .authorize(
                DependencyDecision::UserAuthorizeForTask,
                TASK_ID,
                closure(&wrong_base),
            )
            .unwrap_err()
            .to_string()
            .contains("approved locked image")
    );

    let mut duplicate = request();
    duplicate.packages.push("pkg-config".into());
    assert!(
        duplicate
            .clone()
            .authorize(
                DependencyDecision::UserAuthorizeForTask,
                TASK_ID,
                closure(&duplicate),
            )
            .unwrap_err()
            .to_string()
            .contains("package names are invalid or not canonical")
    );
}

#[test]
fn remote_capability_authenticates_task_manifest_transfer_and_session_key() {
    let key = [0x5a; 32];
    let proposed = request();
    let packages = closure(&proposed);
    let workspace = crate::execution::contract::WorkspaceRef {
        project_id: PROJECT_ID.into(),
        task_id: TASK_ID.into(),
        source_sha: "a".repeat(40),
    };
    let transfer_id = "00000000-0000-4000-8000-000000000140";
    let decision = DependencyDecision::UserAuthorizeForTask;
    let capability = sign_system_image_capability(
        &key,
        transfer_id,
        &proposed,
        decision,
        &workspace,
        &packages,
    )
    .unwrap();
    let valid = |key: &[u8; 32],
                 transfer: &str,
                 proposal: &SystemImageExtensionRequest,
                 workspace: &crate::execution::contract::WorkspaceRef,
                 packages: &[builder::PackageArtifactIdentity]| {
        verify_system_image_capability(
            key,
            &capability,
            transfer,
            proposal,
            decision,
            workspace,
            packages,
        )
    };
    assert!(valid(&key, transfer_id, &proposed, &workspace, &packages));

    let mut changed_proposal = proposed.clone();
    changed_proposal.purpose.push('!');
    assert!(!valid(
        &key,
        transfer_id,
        &changed_proposal,
        &workspace,
        &packages
    ));
    let mut changed_workspace = workspace.clone();
    changed_workspace.source_sha.replace_range(..1, "b");
    assert!(!valid(
        &key,
        transfer_id,
        &proposed,
        &changed_workspace,
        &packages
    ));
    let mut changed_packages = packages.clone();
    changed_packages[0].sha256 = format!("{:x}", Sha256::digest(b"different artifact"));
    assert!(!valid(
        &key,
        transfer_id,
        &proposed,
        &workspace,
        &changed_packages
    ));
    assert!(!valid(
        &key,
        "00000000-0000-4000-8000-000000000141",
        &proposed,
        &workspace,
        &packages
    ));
    assert!(!valid(
        &[0x33; 32],
        transfer_id,
        &proposed,
        &workspace,
        &packages
    ));
}
