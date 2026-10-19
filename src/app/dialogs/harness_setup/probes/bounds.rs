use super::super::HarnessProbe;

const MAX_STATUS_BYTES: usize = 512;
const MAX_PROVIDER_ID_BYTES: usize = 64;
const MAX_VERSION_BYTES: usize = 128;
const MAX_EXECUTABLE_BYTES: usize = 4096;
const MAX_DIAGNOSTIC_BYTES: usize = 2048;
const MAX_MODELS: usize = 500;
const MAX_MODEL_ID_BYTES: usize = 256;

pub(super) fn bound_report(mut report: HarnessProbe) -> HarnessProbe {
    if report.id.is_empty() || report.id.len() > MAX_PROVIDER_ID_BYTES || has_control(&report.id) {
        report.id = "unknown".into();
        report.ready = false;
        report.implementation_available = false;
        report.diagnostic = Some("Provider identifier exceeded discovery limits".into());
    }
    bound_string(&mut report.status, MAX_STATUS_BYTES);
    if report
        .version
        .as_ref()
        .is_some_and(|version| version.len() > MAX_VERSION_BYTES || has_control(version))
    {
        report.version = None;
        report.ready = false;
        report.implementation_available = false;
        append_diagnostic(
            &mut report.diagnostic,
            "Provider version exceeded discovery limits",
        );
    }
    if report
        .executable
        .as_ref()
        .is_some_and(|path| path.len() > MAX_EXECUTABLE_BYTES || has_control(path))
    {
        report.executable = None;
        report.ready = false;
        report.implementation_available = false;
        report.diagnostic = Some("Provider executable path exceeded discovery limits".into());
    }

    let invalid_catalog = report.models.len() > MAX_MODELS
        || report.models.iter().any(|model| {
            model.len() > MAX_MODEL_ID_BYTES
                || !crate::harness::discovery::is_valid_model_identifier(model)
        })
        || report.default_model.as_ref().is_some_and(|model| {
            model.len() > MAX_MODEL_ID_BYTES
                || !crate::harness::discovery::is_valid_model_identifier(model)
        });
    if invalid_catalog {
        report.models.clear();
        report.default_model = None;
        report.model_catalog_fresh = false;
        append_diagnostic(
            &mut report.diagnostic,
            "Model catalog exceeded discovery limits",
        );
    }
    if let Some(diagnostic) = report.diagnostic.as_mut() {
        bound_string(diagnostic, MAX_DIAGNOSTIC_BYTES);
    }
    report
}

fn has_control(value: &str) -> bool {
    value.chars().any(char::is_control)
}

fn bound_string(value: &mut String, max_bytes: usize) {
    if value.len() <= max_bytes {
        return;
    }
    let mut boundary = max_bytes;
    while !value.is_char_boundary(boundary) {
        boundary -= 1;
    }
    value.truncate(boundary);
}

fn append_diagnostic(diagnostic: &mut Option<String>, message: &str) {
    let diagnostic = diagnostic.get_or_insert_with(String::new);
    if !diagnostic.is_empty() {
        diagnostic.push_str("; ");
    }
    diagnostic.push_str(message);
}

#[cfg(test)]
mod tests {
    use super::bound_report;
    use crate::app::dialogs::harness_setup::HarnessProbe;

    fn report() -> HarnessProbe {
        HarnessProbe {
            id: "pi".into(),
            status: "pi 1.0".into(),
            version: Some("1.0".into()),
            executable: Some("/synthetic/bin/pi".into()),
            diagnostic: None,
            ready: true,
            models: vec!["model-a".into()],
            default_model: Some("model-a".into()),
            model_catalog_fresh: true,
            configuration_required: false,
            authentication: crate::execution::contract::ReadinessState::Unknown,
            implementation_available: true,
        }
    }

    #[test]
    fn excessive_catalog_data_fails_catalog_closed_without_hiding_ready_cli() {
        let mut report = report();
        report.models = vec!["model".into(); 501];
        let bounded = bound_report(report);

        assert!(bounded.ready);
        assert!(bounded.implementation_available);
        assert!(bounded.models.is_empty());
        assert!(bounded.default_model.is_none());
        assert!(!bounded.model_catalog_fresh);
        assert!(
            bounded
                .diagnostic
                .as_deref()
                .is_some_and(|message| message.contains("Model catalog exceeded discovery limits"))
        );
    }

    #[test]
    fn oversized_or_controlled_model_identifiers_are_rejected() {
        for model in [
            "m".repeat(257),
            "model\n-injection".into(),
            "openai/gpt token=synthetic-secret".into(),
        ] {
            let mut report = report();
            report.models = vec![model];
            let bounded = bound_report(report);
            assert!(bounded.models.is_empty());
            assert!(!bounded.model_catalog_fresh);
        }
    }

    #[test]
    fn secret_like_default_model_is_removed_before_settings_persistence() {
        let mut report = report();
        report.default_model = Some("gpt-5?token=synthetic-secret".into());
        let bounded = bound_report(report);
        assert!(bounded.models.is_empty());
        assert!(bounded.default_model.is_none());
        assert!(!bounded.model_catalog_fresh);
        assert!(!bounded.status.contains("synthetic-secret"));
        assert!(
            !bounded
                .diagnostic
                .unwrap_or_default()
                .contains("synthetic-secret")
        );
    }

    #[test]
    fn oversized_executable_path_cannot_become_a_ready_provider() {
        let mut report = report();
        report.executable = Some(format!("/{}", "x".repeat(4096)));
        let bounded = bound_report(report);
        assert!(bounded.executable.is_none());
        assert!(!bounded.ready);
        assert!(!bounded.implementation_available);
    }

    #[test]
    fn oversized_version_cannot_be_reported_as_a_ready_provider() {
        let mut report = report();
        report.version = Some("v".repeat(129));
        let bounded = bound_report(report);
        assert!(bounded.version.is_none());
        assert!(!bounded.ready);
        assert!(!bounded.implementation_available);
    }
}
