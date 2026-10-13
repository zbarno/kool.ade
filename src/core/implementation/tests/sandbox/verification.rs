pub(crate) fn install_verification_executor()
-> super::super::super::runner::TestVerificationExecutorGuard {
    super::super::super::runner::install_test_verification_executor(|cwd, command, workspace| {
        workspace.validate()?;
        let output = std::process::Command::new("sh")
            .args(["-c", command])
            .current_dir(cwd)
            .output()?;
        let stdout = String::from_utf8_lossy(&output.stdout);
        let stderr = String::from_utf8_lossy(&output.stderr);
        anyhow::ensure!(
            output.status.success(),
            "Docker verification command failed: {command}\\n{stdout}{stderr}"
        );
        Ok(format!("{stdout}{stderr}"))
    })
}
