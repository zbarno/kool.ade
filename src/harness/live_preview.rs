//! Read-only projections of an incomplete response. Never used for persistence.

/// Separate conversational prose from the structured envelope, including when
/// the fence or a JSON escape arrives across multiple stream chunks.
pub fn project(text: &str) -> (String, Option<String>) {
    let trimmed = text.trim_start();
    let (prose, json) = if trimmed.starts_with('{') {
        ("", Some(trimmed))
    } else if let Some(start) = text.find("```json") {
        (
            text[..start].trim_end(),
            Some(text[start + 7..].trim_start()),
        )
    } else if let Some(start) = text.find("```") {
        return (text[..start].trim_end().to_owned(), None);
    } else {
        // Hold a partial opening fence until we know what follows.
        return (text.trim_end_matches('`').to_owned(), None);
    };
    let Some(json) = json else {
        return (prose.to_owned(), None);
    };
    let assistant = top_level_string(json, &["assistantMessage", "assistant_message"]);
    let spec = top_level_string(json, &["updatedSpecification", "updated_specification"]);
    (
        assistant
            .filter(|s| !s.is_empty())
            .unwrap_or_else(|| prose.to_owned()),
        spec,
    )
}

fn top_level_string(json: &str, keys: &[&str]) -> Option<String> {
    let bytes = json.as_bytes();
    let mut i = 0;
    let mut depth = 0usize;
    let mut expecting_key = false;
    while i < bytes.len() {
        match bytes[i] {
            b'{' | b'[' => {
                depth += 1;
                expecting_key = depth == 1;
                i += 1;
            }
            b'}' | b']' => {
                depth = depth.saturating_sub(1);
                i += 1;
            }
            b',' => {
                expecting_key = depth == 1;
                i += 1;
            }
            b'"' => {
                let (value, used, complete) = json_string(&json[i..])?;
                i += used;
                if !complete {
                    return None;
                }
                if depth == 1 && expecting_key {
                    expecting_key = false;
                    while bytes.get(i).is_some_and(u8::is_ascii_whitespace) {
                        i += 1;
                    }
                    if bytes.get(i) != Some(&b':') {
                        return None;
                    }
                    i += 1;
                    while bytes.get(i).is_some_and(u8::is_ascii_whitespace) {
                        i += 1;
                    }
                    if keys.contains(&value.as_str()) {
                        return json_string(&json[i..]).map(|(s, _, _)| s);
                    }
                }
            }
            _ => i += 1,
        }
    }
    None
}

/// Decode the complete prefix of a JSON string. Incomplete escapes (including
/// surrogate pairs) are withheld, so previews never display raw JSON escapes.
fn json_string(s: &str) -> Option<(String, usize, bool)> {
    let b = s.as_bytes();
    if b.first() != Some(&b'"') {
        return None;
    }
    let mut i = 1;
    let mut safe = 1;
    while i < b.len() {
        match b[i] {
            b'"' => {
                return serde_json::from_str(&s[..=i])
                    .ok()
                    .map(|v| (v, i + 1, true));
            }
            b'\\' => {
                if i + 1 >= b.len() {
                    break;
                }
                if b[i + 1] == b'u' {
                    if i + 6 > b.len() {
                        break;
                    }
                    let code = u16::from_str_radix(s.get(i + 2..i + 6)?, 16).ok()?;
                    let len = if (0xd800..=0xdbff).contains(&code) {
                        12
                    } else {
                        6
                    };
                    if i + len > b.len() {
                        break;
                    }
                    i += len;
                } else {
                    i += 2;
                }
                if !s.is_char_boundary(i) {
                    return None;
                }
                safe = i;
            }
            _ => {
                i += s.get(i..)?.chars().next()?.len_utf8();
                safe = i;
            }
        }
    }
    serde_json::from_str(&format!("{}\"", &s[..safe]))
        .ok()
        .map(|v| (v, s.len(), false))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_chunk_boundary_decodes_without_leaking_json() {
        let spec = "# Café\nQuoted \"scope\", \\ paths and 🚀";
        let text = format!(
            "Drafting.\n```json\n{}\n```",
            serde_json::json!({
                "assistantMessage": "Here is the draft.", "updatedSpecification": spec
            })
        );
        let mut saw_partial = false;
        for (i, _) in text.char_indices() {
            let (chat, preview) = project(&text[..i]);
            assert!(!chat.contains("assistantMessage"));
            if let Some(part) = preview {
                assert!(spec.starts_with(&part));
                saw_partial |= !part.is_empty() && part != spec;
            }
        }
        assert!(saw_partial);
        assert_eq!(project(&text).1.as_deref(), Some(spec));
    }

    #[test]
    fn unicode_escape_pairs_and_snake_case_are_supported() {
        let text = r#"{"updated_specification":"Hi\n\uD83D\uDE80"}"#;
        for i in 0..text.len() {
            if let Some(s) = project(&text[..i]).1 {
                assert!("Hi\n🚀".starts_with(&s));
            }
        }
        assert_eq!(project(text).1.as_deref(), Some("Hi\n🚀"));
    }

    #[test]
    fn null_nested_keys_and_mentions_in_prose_are_not_updates() {
        assert_eq!(project(r#"{"updatedSpecification":null}"#).1, None);
        assert_eq!(project(r#"{"nested":{"updatedSpecification":"wrong"},"assistantMessage":"updatedSpecification"}"#).1, None);
        assert_eq!(project("I will update updatedSpecification soon.").1, None);
    }
    #[test]
    fn malformed_escapes_never_panic_and_code_in_spec_stays_intact() {
        for input in [
            r#"{"updatedSpecification":"\uD800ééé"}"#,
            r#"{"updatedSpecification":"\q"}"#,
        ] {
            assert_eq!(project(input).1, None);
        }
        let spec = "# API\n```json\n{\"hello\": true}\n```";
        let text = format!(
            "Example:\n```text\nhello\n```\n\n```json\n{}\n```",
            serde_json::json!({"updatedSpecification":spec})
        );
        assert_eq!(project(&text).1.as_deref(), Some(spec));
    }
}
