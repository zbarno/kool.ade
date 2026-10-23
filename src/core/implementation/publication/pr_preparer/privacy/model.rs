use super::{SensitiveContentError, email_like, opaque_identifier, trim_token};

pub fn validate_model_description(description: &str) -> anyhow::Result<()> {
    let tokens = description.split_whitespace().collect::<Vec<_>>();
    let contains_sensitive_value = tokens.iter().any(|token| {
        let trimmed = trim_token(token);
        email_like(trimmed)
            || trimmed.chars().filter(char::is_ascii_digit).count() >= 7
            || opaque_identifier(trimmed)
    }) || contains_grouped_phone_number(&tokens);
    anyhow::ensure!(!contains_sensitive_value, SensitiveContentError);
    Ok(())
}

fn contains_grouped_phone_number(tokens: &[&str]) -> bool {
    tokens.windows(3).any(|group| {
        let digits = group
            .iter()
            .map(|token| token.chars().filter(char::is_ascii_digit).count())
            .collect::<Vec<_>>();
        digits == [3, 3, 4]
    }) || tokens.windows(4).any(|group| {
        let digits = group
            .iter()
            .map(|token| token.chars().filter(char::is_ascii_digit).count())
            .collect::<Vec<_>>();
        digits == [1, 3, 3, 4]
    })
}
