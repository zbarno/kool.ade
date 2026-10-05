use super::*;

pub(super) fn already_verified(
    dir: &Path,
    state: &Implementation,
    clean: bool,
    head: &str,
) -> anyhow::Result<bool> {
    let pending =
        crate::core::implementation::initial_reconciliation::pending_required_verification(dir)?;
    Ok(clean
        && state.verified_head.as_deref() == Some(head)
        && crate::core::implementation::initial_reconciliation::verified_report_covers(
            dir, &pending,
        )
        && (!state.auto_merge || dir.join("verified-report.json").exists()))
}

pub(super) fn commands_to_run(dir: &Path, reported: &[String]) -> anyhow::Result<Vec<String>> {
    let required = crate::core::implementation::initial_reconciliation::required_verification(dir)?;
    let mut commands =
        crate::core::implementation::initial_reconciliation::pending_required_verification(dir)?;
    for command in reported {
        append_reported_check(&required, &mut commands, command);
    }
    Ok(commands)
}

fn append_reported_check(required: &[String], commands: &mut Vec<String>, reported: &str) {
    if let Some(required) =
        crate::core::implementation::initial_reconciliation::required_command_for_report(
            required, reported,
        )
    {
        if !commands.iter().any(|command| command == required) {
            commands.push(required.to_owned());
        }
    } else if !commands.iter().any(|command| command == reported) {
        commands.push(reported.to_owned());
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bare_report_uses_unique_scoped_required_check_even_when_not_pending() {
        let required = vec!["cd -- 'Source' && dotnet test".into()];
        let mut commands = Vec::new();

        append_reported_check(&required, &mut commands, "dotnet test");

        assert_eq!(commands, ["cd -- 'Source' && dotnet test"]);
    }

    #[test]
    fn explicit_report_scope_different_from_requirement_is_preserved() {
        let required = vec!["cd -- 'Source' && dotnet test".into()];
        let mut commands = Vec::new();

        append_reported_check(&required, &mut commands, "cd -- 'ClientApp' && dotnet test");

        assert_eq!(commands, ["cd -- 'ClientApp' && dotnet test"]);
    }
}
