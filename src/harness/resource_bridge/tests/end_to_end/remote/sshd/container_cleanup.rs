use std::process::Command;

impl super::SshdFixture {
    pub(in crate::harness::resource_bridge::tests::end_to_end::remote) fn cleanup_worker_containers_for_test(
        &self,
        expected_owner_id: &str,
    ) -> std::io::Result<()> {
        let owner_path = self.root.join("worker-state/executions/owner-instance-id");
        let recorded_owner = std::fs::read_to_string(owner_path)?;
        if recorded_owner.trim() != expected_owner_id {
            return Err(std::io::Error::other(
                "worker container cleanup owner does not match the fixture",
            ));
        }
        remove_owned_containers(expected_owner_id)
    }
}

impl Drop for super::SshdFixture {
    fn drop(&mut self) {
        if let Some(pid_path) = &self.worker_process_pid_file
            && let Some(state) = pid_path.parent()
            && let Ok(owner) = std::fs::read_to_string(state.join("executions/owner-instance-id"))
        {
            let _ = remove_owned_containers(owner.trim());
        }
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

pub(super) fn remove_owned_containers(owner_id: &str) -> std::io::Result<()> {
    let parsed = uuid::Uuid::parse_str(owner_id)
        .map_err(|_| std::io::Error::other("worker container owner ID is invalid"))?;
    if parsed.hyphenated().to_string() != owner_id {
        return Err(std::io::Error::other(
            "worker container owner ID is not canonical",
        ));
    }

    let filter = format!("label=com.koolade.owner={owner_id}");
    for id in list_owned_containers(&filter)? {
        if !(12..=64).contains(&id.len()) || !id.bytes().all(|byte| byte.is_ascii_hexdigit()) {
            return Err(std::io::Error::other(
                "Docker returned an invalid container ID for the worker owner",
            ));
        }
        let output = Command::new("docker")
            .args(["container", "rm", "--force"])
            .arg(&id)
            .output()?;
        if !output.status.success() {
            return Err(std::io::Error::other(format!(
                "could not remove worker-owned Docker container {}: {}",
                id,
                String::from_utf8_lossy(&output.stderr).trim()
            )));
        }
    }
    if !list_owned_containers(&filter)?.is_empty() {
        return Err(std::io::Error::other(
            "worker-owned Docker containers remain after cleanup",
        ));
    }
    Ok(())
}

fn list_owned_containers(filter: &str) -> std::io::Result<Vec<String>> {
    let output = Command::new("docker")
        .args(["container", "ls", "--all", "--quiet", "--filter"])
        .arg(filter)
        .output()?;
    if !output.status.success() {
        return Err(std::io::Error::other(format!(
            "could not list worker-owned Docker containers: {}",
            String::from_utf8_lossy(&output.stderr).trim()
        )));
    }
    String::from_utf8(output.stdout)
        .map_err(std::io::Error::other)
        .map(|stdout| {
            stdout
                .lines()
                .map(str::trim)
                .filter(|line| !line.is_empty())
                .map(str::to_owned)
                .collect()
        })
}
