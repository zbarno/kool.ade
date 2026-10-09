use super::*;

pub(super) fn run(
    runner: &Runner,
    state: &Implementation,
    dir: &Path,
    plan: &VerificationPlan,
    stamp: i64,
    evidence: &mut Vec<serde_json::Value>,
) -> anyhow::Result<Option<String>> {
    let mut report_check_clone = None;
    for command in &plan.commands {
        runner.update(format!("Verifying: {command}"));
        let result = if plan.application_owned.contains(command) {
            initial_reconciliation::support::generated::verify(runner, state, dir, command)
        } else {
            if report_check_clone.is_none() {
                report_check_clone = Some(ReportCheckClone::create(state, dir, runner)?);
            }
            runner.verify(report_check_clone.as_ref().unwrap().path(), command)
        };
        evidence.push(serde_json::json!({
            "command": command,
            "output": result.as_ref().ok().map(|text| crate::error::redact_secrets(text)),
            "error": result.as_ref().err().map(ToString::to_string).map(|text| crate::error::redact_secrets(&text)),
        }));
        crate::artifacts::atomic_write_bytes(
            &dir.join(format!("{stamp}-verification.json")),
            &serde_json::to_vec_pretty(evidence)?,
        )?;
        if let Err(error) = result {
            return Ok(Some(format!(
                "Verification command failed: {command}\n{error}"
            )));
        }
    }
    Ok(None)
}
