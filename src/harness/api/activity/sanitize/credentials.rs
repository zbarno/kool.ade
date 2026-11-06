pub(super) fn redact_sensitive_fields(input: &str) -> String {
    let bytes = input.as_bytes();
    let mut output = String::with_capacity(input.len());
    let mut copied_to = 0;
    let mut cursor = 0;
    while cursor < bytes.len() {
        if !bytes[cursor].is_ascii_alphanumeric() && bytes[cursor] != b'_' && bytes[cursor] != b'-'
        {
            cursor += 1;
            continue;
        }
        let key_start = cursor;
        while cursor < bytes.len()
            && (bytes[cursor].is_ascii_alphanumeric() || matches!(bytes[cursor], b'_' | b'-'))
        {
            cursor += 1;
        }
        let key_end = cursor;
        let key = input[key_start..key_end].to_ascii_lowercase();
        let mut separator = cursor;
        if separator < bytes.len() && matches!(bytes[separator], b'"' | b'\'') {
            separator += 1;
        }
        while separator < bytes.len() && bytes[separator].is_ascii_whitespace() {
            separator += 1;
        }
        if separator >= bytes.len() || !matches!(bytes[separator], b':' | b'=') {
            continue;
        }
        if !is_sensitive_label(&key) {
            continue;
        }
        let mut value_start = separator + 1;
        while value_start < bytes.len() && bytes[value_start].is_ascii_whitespace() {
            value_start += 1;
        }
        if value_start >= bytes.len() {
            continue;
        }
        let quote = matches!(bytes[value_start], b'"' | b'\'').then_some(bytes[value_start]);
        let content_start = value_start + usize::from(quote.is_some());
        let mut value_end = content_start;
        if let Some(quote) = quote {
            let mut escaped = false;
            while value_end < bytes.len() {
                let byte = bytes[value_end];
                if byte == quote && !escaped {
                    break;
                }
                escaped = byte == b'\\' && !escaped;
                if byte != b'\\' {
                    escaped = false;
                }
                value_end += 1;
            }
        } else {
            while value_end < bytes.len()
                && !bytes[value_end].is_ascii_whitespace()
                && !matches!(bytes[value_end], b',' | b'}' | b']' | b'&' | b'#')
            {
                value_end += 1;
            }
            if input[content_start..value_end].eq_ignore_ascii_case("bearer") {
                while value_end < bytes.len() && bytes[value_end].is_ascii_whitespace() {
                    value_end += 1;
                }
                while value_end < bytes.len()
                    && !bytes[value_end].is_ascii_whitespace()
                    && !matches!(bytes[value_end], b',' | b'}' | b']' | b'&' | b'#')
                {
                    value_end += 1;
                }
            }
        }
        if value_end == content_start {
            continue;
        }
        output.push_str(&input[copied_to..content_start]);
        output.push_str("[REDACTED]");
        copied_to = value_end;
        cursor = value_end;
    }
    output.push_str(&input[copied_to..]);
    output
}

pub(super) fn is_sensitive_label(label: &str) -> bool {
    [
        "token",
        "access_token",
        "secret",
        "password",
        "api_key",
        "apikey",
        "api-key",
        "access_key",
        "authorization",
        "private_key",
        "privatekey",
        "client_secret",
        "clientsecret",
        "refresh_token",
        "refreshtoken",
    ]
    .iter()
    .any(|known| label == *known || label.ends_with(&format!("_{known}")))
}
