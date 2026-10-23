pub(super) fn approved_driver_mode(driver: &str, args: &[String]) -> Option<&'static str> {
    match (driver, args) {
        ("browser" | "service", []) | ("desktop", []) => Some("default"),
        ("desktop", [arg]) if arg == "--fixture-smoke" => Some("fixture-smoke"),
        ("desktop", [arg]) if arg == "--fixture-cache-smoke" => Some("fixture-cache-smoke"),
        _ => None,
    }
}

pub(super) fn approved_driver_command(
    driver: Option<&str>,
    mode: Option<&str>,
) -> Option<Vec<String>> {
    match (driver?, mode?) {
        ("browser" | "service", "default") => Some(vec![
            "node".into(),
            "/opt/koolade-runtime-qa/driver.mjs".into(),
        ]),
        ("desktop", "default") => Some(vec!["/usr/local/bin/koolade-qa-desktop".into()]),
        ("desktop", "fixture-smoke") => Some(vec![
            "/usr/local/bin/koolade-qa-desktop".into(),
            "--fixture-smoke".into(),
        ]),
        ("desktop", "fixture-cache-smoke") => Some(vec![
            "/usr/local/bin/koolade-qa-desktop".into(),
            "--fixture-cache-smoke".into(),
        ]),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::{approved_driver_command, approved_driver_mode};

    #[test]
    fn service_driver_uses_the_locked_browser_runtime_entrypoint() {
        assert_eq!(approved_driver_mode("service", &[]), Some("default"));
        assert_eq!(
            approved_driver_command(Some("service"), Some("default")),
            Some(vec![
                "node".into(),
                "/opt/koolade-runtime-qa/driver.mjs".into()
            ])
        );
    }

    #[test]
    fn driver_modes_reject_unapproved_arguments_and_combinations() {
        assert_eq!(
            approved_driver_mode("service", &["--anything".into()]),
            None
        );
        assert_eq!(
            approved_driver_command(Some("service"), Some("fixture-smoke")),
            None
        );
        assert_eq!(
            approved_driver_command(Some("unknown"), Some("default")),
            None
        );
    }
}
