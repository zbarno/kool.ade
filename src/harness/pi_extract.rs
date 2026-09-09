//! Extraction of the structured response from the agent's final message.
//!
//! Contract with the agent (enforced in `core::prompt`): the final message
//! ends with ONE fenced ```json block carrying the turn envelope object.
//! Extraction rules:
//! * prefer the LAST fenced json block (agent prose may show drafts earlier)
//! * if no fence exists, try the whole trimmed text when it is a lone object
//! * balanced-brace scanning is string-aware (braces inside quotes ignored)
//!
/// Find the JSON object substring inside agent text. Returns `None` when no
/// plausible object exists.
pub fn extract_json_object(text: &str) -> Option<String> {
    // Scan the JSON object before looking for its closing Markdown fence.
    // Task stories and specifications may themselves contain fenced code.
    let mut last = None;
    let mut i = 0;
    while let Some(found) = find_needle(text, i, "```json") {
        let content_start = found + 7;
        let inner = text[content_start..].trim_start();
        if let Some(end) = object_end(inner) {
            let tail = inner[end..].trim_start();
            if tail.starts_with("```") {
                last = Some(inner[..end].to_owned());
                i = text.len() - tail.len() + 3;
                continue;
            }
        }
        i = content_start;
    }
    if last.is_some() {
        return last;
    }
    // 2) Whole-message bare object.
    let trimmed = text.trim();
    if trimmed.starts_with('{') && trimmed.ends_with('}') {
        return balanced_object(trimmed).map(str::to_string);
    }
    None
}

/// Return the span of the first balanced `{...}` object whose braces are
/// string-aware. `None` if unbalanced.
pub fn balanced_object(json: &str) -> Option<&str> {
    let start = json.find('{')?;
    let end = start + object_end(&json[start..])?;
    json[end..].trim().is_empty().then_some(&json[start..end])
}

fn object_end(json: &str) -> Option<usize> {
    if !json.starts_with('{') {
        return None;
    }
    let mut depth = 0usize;
    let mut in_string = false;
    let mut escape = false;
    for (idx, ch) in json.char_indices() {
        match ch {
            _ if escape => escape = false,
            '"' => in_string = !in_string,
            '\\' if in_string => escape = true,
            '{' if !in_string => depth += 1,
            '}' if !in_string => {
                depth = depth.checked_sub(1)?;
                if depth == 0 {
                    return Some(idx + 1);
                }
            }
            _ => {}
        }
    }
    None
}

/// Small fixed-needle finder (no regex dependency).
fn find_needle(haystack: &str, from: usize, needle: &str) -> Option<usize> {
    haystack[from..].find(needle).map(|p| from + p)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn wrap(body: &str) -> String {
        format!("Talking as usual...\n\n```json\n{body}\n```\nDone.")
    }

    #[test]
    fn extracts_simple_envelope() {
        let t = wrap(r#"{"schemaVersion":1,"assistantMessage":"hi"}"#);
        let obj = extract_json_object(&t).expect("found");
        let v: serde_json::Value = serde_json::from_str(&obj).unwrap();
        assert_eq!(v["assistantMessage"], "hi");
    }

    #[test]
    fn braces_inside_strings_do_not_confuse_balancing() {
        let t = wrap(r#"{"assistantMessage":"said {this, that} literally"}"#);
        let obj = extract_json_object(&t).expect("found");
        assert!(obj.contains("this, that"));
    }

    #[test]
    fn escaped_quotes_in_strings_handled() {
        let t = wrap(r#"{"question":"He said \"}\" twice"}"#);
        let obj = extract_json_object(&t).expect("found");
        let v: serde_json::Value = serde_json::from_str(&obj).unwrap();
        assert_eq!(v["question"].as_str(), Some("He said \"}\" twice"));
    }

    #[test]
    fn last_fence_wins_over_drafts() {
        let t = format!(
            "Draft:\n```json\n{{\"assistantMessage\":\"WRONG draft\"}}\n```\nFinal:\n```json\n{{\"assistantMessage\":\"RIGHT\"}}\n```"
        );
        let obj = extract_json_object(&t).expect("found");
        assert!(obj.contains("RIGHT"));
        assert!(!obj.contains("WRONG"));
    }

    #[test]
    fn bare_object_without_fences_accepted() {
        let t = "Sure.\n{\"schemaVersion\":1,\"assistantMessage\":\"bare\"}\nok?";
        assert!(extract_json_object(t).is_none()); // trailing prose after object → reject (strict)
        let t2 = "{\"schemaVersion\":1,\"assistantMessage\":\"bare\"}";
        assert!(extract_json_object(t2).is_some());
    }

    #[test]
    fn unbalanced_input_yields_none() {
        assert!(balanced_object("{\"a\":{\"b\":1").is_none());
        assert!(balanced_object("no object at all").is_none());
        assert!(extract_json_object("```json\nbroken\n```").is_none());
    }
    #[test]
    fn story_code_fences_do_not_end_the_response_envelope() {
        let code = "Example:\n```json\n{\"flag\": true}\n```\nNext step.";
        let value = serde_json::json!({"task_stories":[{"implementation_steps":[code]}]});
        let text = format!("```json\n{value}\n```");
        assert_eq!(
            serde_json::from_str::<serde_json::Value>(&extract_json_object(&text).unwrap())
                .unwrap(),
            value
        );
    }
}
