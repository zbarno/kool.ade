use crate::ui::task_detail::RuntimeRecipeOperation;

pub(super) struct Suggestion {
    pub label: &'static str,
    pub executable_id: &'static str,
    pub argv: &'static [&'static str],
}

pub(super) fn suggestion(
    language: Option<&str>,
    operation: RuntimeRecipeOperation,
) -> Option<Suggestion> {
    let (label, executable_id, argv) = match (language?, operation) {
        ("Rust", RuntimeRecipeOperation::Build) => ("Cargo", "cargo", &["build", "--locked"][..]),
        ("Rust", RuntimeRecipeOperation::Test) => ("Cargo", "cargo", &["test", "--locked"][..]),
        ("JavaScript", RuntimeRecipeOperation::Build) => ("Node", "npm", &["run", "build"][..]),
        ("JavaScript", RuntimeRecipeOperation::Test) => ("Node", "npm", &["test"][..]),
        ("JavaScript", RuntimeRecipeOperation::Start) => ("Node", "npm", &["run", "start"][..]),
        ("Python", RuntimeRecipeOperation::Test) => ("Python", "python", &["-m", "pytest"][..]),
        ("Go", RuntimeRecipeOperation::Test) => ("Go", "go", &["test", "./..."][..]),
        _ => return None,
    };
    Some(Suggestion {
        label,
        executable_id,
        argv,
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
                Operation::Build,
                "cargo",
                ["build", "--locked"].as_slice(),
            ),
            (
                "JavaScript",
                Operation::Start,
                "npm",
                ["run", "start"].as_slice(),
            ),
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
