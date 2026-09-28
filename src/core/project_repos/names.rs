use super::Repository;
use unicode_general_category::{GeneralCategory, get_general_category};

pub fn display_labels(repositories: &[Repository]) -> Vec<String> {
    let bases: Vec<_> = repositories.iter().map(base_label).collect();
    let mut counts = std::collections::HashMap::<String, usize>::new();
    for base in &bases {
        *counts.entry(base.clone()).or_default() += 1;
    }
    repositories
        .iter()
        .zip(bases)
        .map(|(repository, base)| {
            let missing_name = repository
                .display_name
                .as_deref()
                .is_none_or(|name| name.trim().is_empty());
            if missing_name || counts.get(&base).copied().unwrap_or_default() > 1 {
                format!("{base} ({})", repository.id)
            } else {
                base
            }
        })
        .collect()
}

fn base_label(repository: &Repository) -> String {
    repository
        .display_name
        .as_deref()
        .filter(|name| !name.trim().is_empty())
        .or_else(|| (!repository.role.trim().is_empty()).then_some(repository.role.as_str()))
        .unwrap_or(&repository.id)
        .to_owned()
}

/// Trim and validate an operator-facing repository name.
///
/// Empty or whitespace-only values mean the repository has no custom label.
/// Names are capped by Unicode scalar count and cannot contain invisible
/// formatting or control characters.
pub fn normalize_display_name(raw: &str) -> anyhow::Result<Option<String>> {
    let name = raw.trim();
    if name.is_empty() {
        return Ok(None);
    }
    anyhow::ensure!(
        name.chars().count() <= 40,
        "Repository display name must be 40 characters or fewer"
    );
    anyhow::ensure!(
        !name.chars().any(|character| {
            matches!(
                get_general_category(character),
                GeneralCategory::Control | GeneralCategory::Format
            )
        }),
        "Repository display name cannot contain invisible or control characters"
    );
    Ok(Some(name.to_owned()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn names_are_trimmed_and_empty_names_are_unset() {
        assert_eq!(
            normalize_display_name("  API Team  ").unwrap(),
            Some("API Team".into())
        );
        assert_eq!(normalize_display_name(" \t ").unwrap(), None);
    }

    #[test]
    fn names_reject_long_and_control_character_values() {
        assert!(
            normalize_display_name(&"x".repeat(41))
                .unwrap_err()
                .to_string()
                .contains("40")
        );
        assert!(
            normalize_display_name("Repo\u{1b}Name")
                .unwrap_err()
                .to_string()
                .contains("control")
        );
        assert!(
            normalize_display_name("API\u{200b}Team")
                .unwrap_err()
                .to_string()
                .contains("invisible")
        );
    }

    #[test]
    fn labels_fallback_to_role_and_append_id_for_missing_or_duplicate_names() {
        let repositories = vec![
            Repository {
                id: "api-a".into(),
                role: "Backend".into(),
                remote: "remote-a".into(),
                display_name: Some("API".into()),
            },
            Repository {
                id: "api-b".into(),
                role: "Backend".into(),
                remote: "remote-b".into(),
                display_name: Some("API".into()),
            },
            Repository {
                id: "worker".into(),
                role: "Worker".into(),
                remote: "remote-c".into(),
                display_name: None,
            },
            Repository {
                id: "solo".into(),
                role: "Other".into(),
                remote: "remote-d".into(),
                display_name: Some("Solo".into()),
            },
        ];
        assert_eq!(
            display_labels(&repositories),
            ["API (api-a)", "API (api-b)", "Worker (worker)", "Solo"]
        );
    }
}
