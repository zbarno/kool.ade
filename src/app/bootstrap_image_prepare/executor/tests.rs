use crate::app::bootstrap_repair_policy::Host;

#[test]
fn image_proposal_binds_the_current_host() {
    let task_id = format!("setup:images:general:arm64:{}", "a".repeat(64));
    let proposal = super::proposal(&task_id).unwrap();
    #[cfg(target_os = "linux")]
    assert_eq!(proposal.host, Host::Linux);
    #[cfg(target_os = "macos")]
    assert_eq!(proposal.host, Host::MacOs);
    #[cfg(target_os = "windows")]
    assert_eq!(proposal.host, Host::Windows);
    assert_eq!(proposal.prerequisite_id, task_id);
    assert_eq!(proposal.image_profile.as_deref(), Some("general"));
}

#[test]
fn image_proposal_rejects_noncanonical_task_ids() {
    assert!(super::proposal("setup:images:general:amd64:short").is_err());
    assert!(super::proposal(&format!("setup:images:browser_qa:amd64:{}", "a".repeat(64))).is_err());
}
