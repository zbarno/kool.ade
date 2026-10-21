use crate::{
    project_runtime::{RuntimeOperation, trusted_adapter_template},
    ui::task_detail::RuntimeRecipeOperation,
};

pub(super) struct Suggestion {
    pub label: &'static str,
    pub executable_id: &'static str,
    pub argv: &'static [&'static str],
}

pub(super) fn suggestion(
    language: Option<&str>,
    operation: RuntimeRecipeOperation,
) -> Option<Suggestion> {
    let runtime_operation = match operation {
        RuntimeRecipeOperation::Build => RuntimeOperation::Build,
        RuntimeRecipeOperation::Test => RuntimeOperation::Test,
        RuntimeRecipeOperation::Start => RuntimeOperation::Start,
    };
    let template = trusted_adapter_template(language, runtime_operation)?;
    let label = match template.executable_id {
        "cargo" => "Cargo",
        "npm" => "Node",
        "python" => "Python",
        "go" => "Go",
        _ => return None,
    };
    Some(Suggestion {
        label,
        executable_id: template.executable_id,
        argv: template.argv,
    })
}

#[cfg(test)]
mod tests {
    use super::suggestion;
    use crate::ui::task_detail::RuntimeRecipeOperation as Operation;

    #[test]
    fn common_ecosystem_templates_are_explicit_argv() {
        let cases = [
            (
                "Rust",
                Operation::Test,
                "cargo",
                ["test", "--locked"].as_slice(),
            ),
            ("JavaScript", Operation::Test, "npm", ["test"].as_slice()),
            (
                "Python",
                Operation::Test,
                "python",
                ["-m", "pytest"].as_slice(),
            ),
            ("Go", Operation::Test, "go", ["test", "./..."].as_slice()),
        ];
        for (language, operation, executable, argv) in cases {
            let suggestion = suggestion(Some(language), operation).unwrap();
            assert_eq!(suggestion.executable_id, executable);
            assert_eq!(suggestion.argv, argv);
        }
    }

    #[test]
    fn unknown_stacks_and_unsupported_operations_keep_manual_recipe_path() {
        assert!(suggestion(Some("Odin"), Operation::Build).is_none());
        assert!(suggestion(Some("Rust"), Operation::Start).is_none());
        assert!(suggestion(None, Operation::Test).is_none());
    }
}
