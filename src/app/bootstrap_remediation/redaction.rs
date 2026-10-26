const REDACTED: &str = "[redacted]";
const SENSITIVE_KEYS: &[&str] = &[
    "password",
    "passwd",
    "passphrase",
    "token",
    "accesstoken",
    "refreshtoken",
    "authtoken",
    "idtoken",
    "sessiontoken",
    "secret",
    "clientsecret",
    "secretkey",
    "secretaccesskey",
    "awssecretaccesskey",
    "apikey",
    "xapikey",
    "accesskey",
    "xaccesskey",
    "awsaccesskeyid",
    "privatekey",
    "authorization",
    "cookie",
    "setcookie",
    "signature",
    "sig",
    "credential",
    "credentials",
];

/// Redact common credential fields in process output and persisted diagnostics.
pub(crate) fn redact_keyed_values(value: &str) -> String {
    let mut output = value.to_owned();
    let mut cursor = 0;
    while let Some((content_start, value_end)) = sensitive_value(&output, cursor) {
        output.replace_range(content_start..value_end, REDACTED);
        cursor = content_start + REDACTED.len();
        if cursor >= output.len() {
            break;
        }
    }
    output
}

fn sensitive_value(value: &str, cursor: usize) -> Option<(usize, usize)> {
    for (start, character) in value.char_indices().filter(|(start, _)| *start >= cursor) {
        if !is_key_character(character)
            || value[..start]
                .chars()
                .next_back()
                .is_some_and(is_key_character)
        {
            continue;
        }
        let key_end = value[start..]
            .char_indices()
            .find(|(_, character)| !is_key_character(*character))
            .map_or(value.len(), |(offset, _)| start + offset);
        let normalized = normalize_key(&value[start..key_end]);
        let bearing = normalized == "bearer";
        if !bearing && !is_sensitive_key(&normalized) {
            continue;
        }

        let mut separator = key_end;
        if let Some(quote @ ('\'' | '"')) = value[separator..].chars().next() {
            separator += quote.len_utf8();
        }
        while value[separator..]
            .chars()
            .next()
            .is_some_and(char::is_whitespace)
        {
            separator += value[separator..].chars().next()?.len_utf8();
        }

        let delimiter = value[separator..].chars().next();
        let is_bearer = bearing && delimiter.is_some_and(char::is_whitespace);
        if !is_bearer && !matches!(delimiter, Some(':' | '=')) {
            continue;
        }
        let mut value_start = separator;
        if !is_bearer {
            value_start += delimiter?.len_utf8();
        }
        while value[value_start..]
            .chars()
            .next()
            .is_some_and(char::is_whitespace)
        {
            value_start += value[value_start..].chars().next()?.len_utf8();
        }

        let first = value[value_start..].chars().next()?;
        if matches!(first, '\'' | '"') {
            let content_start = value_start + first.len_utf8();
            return Some((content_start, quoted_value_end(value, content_start, first)));
        }
        if is_bearer {
            let value_end = value[value_start..]
                .find(|character: char| {
                    character.is_whitespace() || matches!(character, ',' | ';' | '}' | ']')
                })
                .map_or(value.len(), |length| value_start + length);
            return Some((value_start, value_end));
        }

        let line_end = value[value_start..]
            .find('\n')
            .map_or(value.len(), |length| value_start + length);
        if delimiter == Some('=') {
            return Some((value_start, line_end));
        }
        if matches!(first, '|' | '>') {
            return Some((value_start, value.len()));
        }
        return Some((value_start, line_end));
    }
    None
}

fn is_sensitive_key(normalized: &str) -> bool {
    SENSITIVE_KEYS
        .iter()
        .any(|sensitive_key| normalized.ends_with(sensitive_key))
}

fn is_key_character(character: char) -> bool {
    character.is_ascii_alphanumeric() || matches!(character, '_' | '-')
}

fn normalize_key(key: &str) -> String {
    key.chars()
        .filter(char::is_ascii_alphanumeric)
        .map(|character| character.to_ascii_lowercase())
        .collect()
}

fn quoted_value_end(value: &str, start: usize, quote: char) -> usize {
    let mut escaped = false;
    let mut characters = value[start..].char_indices().peekable();
    while let Some((offset, character)) = characters.next() {
        if escaped {
            escaped = false;
        } else if quote == '"' && character == '\\' {
            escaped = true;
        } else if character == quote {
            if quote == '\'' && characters.peek().is_some_and(|(_, next)| *next == '\'') {
                characters.next();
            } else {
                return start + offset;
            }
        }
    }
    value.len()
}
