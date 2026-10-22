pub(crate) fn sanitized_reason(reason: &str) -> String {
    let mut output = String::new();
    let mut length = 0;
    let tokens = reason.split_whitespace().collect::<Vec<_>>();
    for (index, token) in tokens.iter().copied().enumerate() {
        let lower = token.to_ascii_lowercase();
        let sensitive_key = has_sensitive_key_separator(token)
            || (index > 0
                && tokens[index - 1].eq_ignore_ascii_case("api")
                && has_key_separator(token, "key"));
        let sensitive_label = has_sensitive_label_before_value(&tokens, index);
        let sensitive = token.starts_with('/')
            || token.contains('/')
            || token.contains('\\')
            || token.contains('@')
            || token.contains("://")
            || sensitive_key
            || sensitive_label
            || lower == "bearer";
        let redact_remainder = sensitive_key || sensitive_label || lower == "bearer";
        let safe = if sensitive { "[redacted]" } else { token };
        if !output.is_empty() {
            output.push(' ');
            length += 1;
        }
        for character in safe.chars().filter(|character| !character.is_control()) {
            if length >= 512 {
                break;
            }
            output.push(character);
            length += 1;
        }
        if redact_remainder {
            break;
        }
        if length >= 512 {
            break;
        }
    }
    if output.is_empty() {
        "isolated QA driver failed before producing observations".into()
    } else {
        output
    }
}

pub(crate) fn sanitized_field(key: &str, value: &str) -> (String, String) {
    let sensitive = is_sensitive_field_name(key) || has_sensitive_key_separator(key);
    let safe_key = sanitized_reason(key);
    let safe_value = if sensitive {
        "[redacted]".into()
    } else {
        sanitized_reason(value)
    };
    (safe_key, safe_value)
}

fn has_sensitive_label_before_value(tokens: &[&str], index: usize) -> bool {
    let Some(next) = tokens.get(index + 1) else {
        return false;
    };
    let label = normalize_key_token(tokens[index]);
    let separator_follows = next.starts_with(':') || next.starts_with('=');
    (is_sensitive_key(&label)
        || (label == "key" && index > 0 && tokens[index - 1].eq_ignore_ascii_case("api")))
        && separator_follows
}

fn has_sensitive_key_separator(token: &str) -> bool {
    separated_keys(token).any(|key| is_sensitive_key(&key))
}

fn has_key_separator(token: &str, expected_key: &str) -> bool {
    separated_keys(token).any(|key| key == expected_key)
}

fn is_sensitive_field_name(field: &str) -> bool {
    is_sensitive_key(&normalize_key_token(field))
}

fn separated_keys(token: &str) -> impl Iterator<Item = String> + '_ {
    token.char_indices().filter_map(|(index, separator)| {
        if !matches!(separator, ':' | '=') {
            return None;
        }
        let key = token[..index]
            .trim_end_matches(['"', '\'', '`'])
            .rsplit(|character: char| {
                !character.is_ascii_alphanumeric() && !matches!(character, '_' | '-')
            })
            .next()
            .map(normalize_sensitive_key)
            .unwrap_or_default();
        Some(key)
    })
}

fn normalize_key_token(token: &str) -> String {
    normalize_sensitive_key(token.trim_matches(|character: char| {
        !character.is_ascii_alphanumeric() && !matches!(character, '_' | '-')
    }))
}

fn is_sensitive_key(key: &str) -> bool {
    matches!(
        key,
        "authorization"
            | "proxy_authorization"
            | "cookie"
            | "set_cookie"
            | "x_api_key"
            | "x_auth_token"
            | "token"
            | "access_token"
            | "refresh_token"
            | "id_token"
            | "session_token"
            | "api_key"
            | "password"
            | "passwd"
            | "secret"
            | "client_secret"
            | "private_key"
            | "credential"
            | "credentials"
    )
}

fn normalize_sensitive_key(key: &str) -> String {
    let characters = key.chars().collect::<Vec<_>>();
    let mut normalized = String::with_capacity(key.len() + 4);
    for (index, character) in characters.iter().copied().enumerate() {
        if character.is_ascii_uppercase()
            && index > 0
            && (characters[index - 1].is_ascii_lowercase()
                || characters[index - 1].is_ascii_digit()
                || (characters[index - 1].is_ascii_uppercase()
                    && characters
                        .get(index + 1)
                        .is_some_and(|next| next.is_ascii_lowercase())))
            && !normalized.ends_with('_')
        {
            normalized.push('_');
        }
        normalized.push(if character == '-' || character.is_whitespace() {
            '_'
        } else {
            character.to_ascii_lowercase()
        });
    }
    normalized
}
