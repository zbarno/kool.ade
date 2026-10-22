/// Production boundary: follows an approved file through SSH staging and
/// rootless Docker, then checks worker/controller journals, returned output,
/// artifacts, and task-clone stage-out for the synthetic secret. Unit tests
/// cannot prove this cross-process boundary. Requires Linux, rootless Docker,
/// loopback sshd, and the worker binary; setup cost is moderate (one worker and
/// one task clone).
#[test]
fn ssh_worker_suppresses_private_file_output_and_discards_workspace_results() {
    super::run_private_file_grant_e2e(super::GrantTermination::Completed);
}
