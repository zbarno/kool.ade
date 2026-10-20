use super::RuntimeOperation;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct TrustedAdapterTemplate {
    pub id: &'static str,
    pub executable_id: &'static str,
    pub argv: &'static [&'static str],
    pub prerequisite_tool_ids: &'static [&'static str],
}

/// Returns a bounded built-in test recipe only for a language confirmed at the
/// selected target root. Build and start commands stay user-approved.
pub(crate) fn trusted_adapter_template(
    language: Option<&str>,
    operation: RuntimeOperation,
) -> Option<TrustedAdapterTemplate> {
    if operation != RuntimeOperation::Test {
        return None;
    }
    let (id, executable_id, argv, prerequisite_tool_ids) = match language? {
        "Rust" => (
            "cargo-test-v1",
            "cargo",
            &["test", "--locked"][..],
            &["cargo"][..],
        ),
        "JavaScript" => ("npm-test-v1", "npm", &["test"][..], &["node", "npm"][..]),
        "Python" => (
            "python-pytest-v1",
            "python",
            &["-m", "pytest"][..],
            &["python"][..],
        ),
        "Go" => ("go-test-v1", "go", &["test", "./..."][..], &["go"][..]),
        _ => return None,
    };
    Some(TrustedAdapterTemplate {
        id,
        executable_id,
        argv,
        prerequisite_tool_ids,
    })
}

#[cfg(test)]
mod tests {
    use super::trusted_adapter_template;
    use crate::project_runtime::RuntimeOperation;

    #[test]
    fn trusted_test_templates_are_bounded_and_stack_specific() {
        for (language, executable, argv) in [
            ("Rust", "cargo", ["test", "--locked"].as_slice()),
            ("JavaScript", "npm", ["test"].as_slice()),
            ("Python", "python", ["-m", "pytest"].as_slice()),
            ("Go", "go", ["test", "./..."].as_slice()),
        ] {
            let template =
                trusted_adapter_template(Some(language), RuntimeOperation::Test).unwrap();
            assert_eq!(template.executable_id, executable);
            assert_eq!(template.argv, argv);
        }
    }

    #[test]
    fn unknown_stack_and_non_test_operation_have_no_adapter() {
        assert!(trusted_adapter_template(Some("Odin"), RuntimeOperation::Test).is_none());
        assert!(trusted_adapter_template(Some("Rust"), RuntimeOperation::Build).is_none());
        assert!(trusted_adapter_template(None, RuntimeOperation::Test).is_none());
    }
}
