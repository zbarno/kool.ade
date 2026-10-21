use super::ProposalScope;
use std::collections::BTreeSet;

const SENSITIVE_CONTEXT: [&str; 16] = [
    "address",
    "account",
    "client",
    "confidential",
    "credential",
    "customer",
    "dob",
    "email",
    "employee",
    "internal",
    "name",
    "patient",
    "phone",
    "private",
    "secret",
    "ssn",
];
const CONTEXT_BOUNDARIES: &[&str] = &[
    "a",
    "about",
    "after",
    "and",
    "are",
    "asked",
    "before",
    "but",
    "by",
    "can",
    "could",
    "for",
    "from",
    "had",
    "has",
    "in",
    "is",
    "may",
    "needs",
    "on",
    "or",
    "our",
    "please",
    "reported",
    "requests",
    "requested",
    "said",
    "should",
    "the",
];

pub(super) fn validate_public_description(
    description: &str,
    scope: &ProposalScope,
) -> anyhow::Result<()> {
    let sources = [
        scope.feature_plan.as_str(),
        scope.task_spec.as_str(),
        scope.gate_evidence.as_str(),
        scope.verified_report.as_str(),
        scope.diff.as_str(),
        scope.scope_manifest.as_str(),
    ];
    let normalized_description = normalize(description);
    let sensitive_fragments = sources
        .into_iter()
        .flat_map(sensitive_fragments)
        .collect::<BTreeSet<_>>();
    let leaked_fragment = sensitive_fragments
        .into_iter()
        .find(|fragment| normalized_description.contains(fragment));
    leaked_fragment.map_or(Ok(()), |_| Err(SensitiveContentError.into()))
}

#[derive(Debug)]
pub(super) struct SensitiveContentError;

impl std::fmt::Display for SensitiveContentError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(
            "PR description repeats a sensitive value or private-context fragment; revise it before public publication",
        )
    }
}

impl std::error::Error for SensitiveContentError {}

pub(super) fn is_sensitive_content_error(error: &anyhow::Error) -> bool {
    error.downcast_ref::<SensitiveContentError>().is_some()
}

fn sensitive_fragments(source: &str) -> Vec<String> {
    let mut fragments = Vec::new();
    for token in source.split_whitespace() {
        let trimmed = token.trim_matches(|character: char| {
            matches!(
                character,
                '<' | '>' | '(' | ')' | '[' | ']' | '{' | '}' | '"' | '\''
            )
        });
        if email_like(trimmed) {
            fragments.push(normalize(trimmed));
        }
        let digits = trimmed
            .chars()
            .filter(char::is_ascii_digit)
            .collect::<String>();
        if digits.len() >= 7 {
            fragments.push(digits);
        }
        if opaque_identifier(trimmed) {
            fragments.push(normalize(trimmed));
        }
    }

    for line in source.lines() {
        let normalized_line = normalize(line);
        if normalized_line.len() >= 24 && contains_sensitive_context(line) {
            fragments.push(normalized_line);
        }
        fragments.extend(contextual_value_fragments(line));
        if let Some(value) = labeled_sensitive_value(line) {
            let normalized_value = normalize(value);
            if !normalized_value.is_empty() {
                fragments.push(normalized_value);
            }
            let words = value
                .split(|character: char| !character.is_ascii_alphanumeric())
                .filter(|word| !word.is_empty())
                .collect::<Vec<_>>();
            for start in 0..words.len() {
                for end in start + 2..=words.len() {
                    let fragment = normalize(&words[start..end].join(" "));
                    if fragment.len() >= 5 {
                        fragments.push(fragment);
                    }
                }
            }
        }
    }
    fragments
        .into_iter()
        .filter(|fragment| !fragment.is_empty())
        .collect()
}

fn contextual_value_fragments(line: &str) -> Vec<String> {
    let words = line
        .split(|character: char| !character.is_ascii_alphanumeric())
        .filter(|word| !word.is_empty())
        .collect::<Vec<_>>();
    let mut fragments = Vec::new();
    for (index, word) in words.iter().enumerate() {
        if !SENSITIVE_CONTEXT
            .iter()
            .any(|sensitive_word| word.eq_ignore_ascii_case(sensitive_word))
        {
            continue;
        }
        let value_words = words[index + 1..]
            .iter()
            .take_while(|value_word| !is_context_boundary(value_word))
            .take(3)
            .copied()
            .collect::<Vec<_>>();
        let has_name_cue = value_words.iter().any(|word| title_case_or_number(word))
            || (index == 0
                && value_words.len() >= 2
                && value_words
                    .iter()
                    .all(|word| word.chars().all(|character| character.is_ascii_lowercase())));
        if value_words.len() >= 2 && has_name_cue {
            for start in 0..value_words.len() {
                for end in start + 2..=value_words.len() {
                    let fragment = normalize(&value_words[start..end].join(" "));
                    if fragment.len() >= 5 {
                        fragments.push(fragment);
                    }
                }
            }
        }
        for start in 0..value_words.len() {
            for end in start + 1..=value_words.len() {
                if end != start + 1 || !title_case_or_number(value_words[start]) {
                    continue;
                }
                let fragment = normalize(&value_words[start..end].join(" "));
                if fragment.len() >= 3 {
                    fragments.push(fragment);
                }
            }
        }
    }
    fragments
}

fn is_context_boundary(word: &str) -> bool {
    CONTEXT_BOUNDARIES
        .iter()
        .any(|boundary| word.eq_ignore_ascii_case(boundary))
}

fn title_case_or_number(word: &str) -> bool {
    let mut characters = word.chars();
    characters
        .next()
        .is_some_and(|first| first.is_ascii_uppercase() || first.is_ascii_digit())
        && (word.chars().any(|character| character.is_ascii_digit())
            || word.chars().any(|character| character.is_ascii_lowercase()))
}

fn labeled_sensitive_value(line: &str) -> Option<&str> {
    let delimiter = line.find([':', '='])?;
    let label = &line[..delimiter];
    let has_sensitive_label = label
        .split(|character: char| !character.is_ascii_alphanumeric())
        .any(|word| {
            SENSITIVE_CONTEXT
                .iter()
                .any(|sensitive_word| word.eq_ignore_ascii_case(sensitive_word))
        });
    has_sensitive_label.then(|| line[delimiter + 1..].trim())
}

fn email_like(value: &str) -> bool {
    let Some((local, domain)) = value.split_once('@') else {
        return false;
    };
    !local.is_empty() && domain.contains('.') && !domain.starts_with('.') && !domain.ends_with('.')
}

fn opaque_identifier(value: &str) -> bool {
    value.len() >= 16
        && value
            .chars()
            .any(|character| character.is_ascii_alphabetic())
        && value.chars().any(|character| character.is_ascii_digit())
        && value
            .chars()
            .all(|character| character.is_ascii_alphanumeric() || "_-./".contains(character))
}

fn contains_sensitive_context(value: &str) -> bool {
    let lower = value.to_ascii_lowercase();
    SENSITIVE_CONTEXT.iter().any(|word| lower.contains(word))
}

fn normalize(value: &str) -> String {
    value
        .chars()
        .filter(char::is_ascii_alphanumeric)
        .flat_map(char::to_lowercase)
        .collect()
}

#[cfg(test)]
#[path = "privacy_tests.rs"]
mod tests;
