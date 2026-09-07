//! Extraction of the structured response from the agent's final message.
//!
//! Contract with the agent (enforced in `core::prompt`): the final message
//! ends with ONE fenced ```json block carrying the turn envelope object.
//! Extraction rules:
//! * prefer the LAST fenced json block (agent prose may show drafts earlier)
//! * if no fence exists, try the whole trimmed text when it is a lone object
//! * balanced-brace scanning is string-aware (braces inside quotes ignored)
//!
//! Limitation: the escape pass treats the single character after a backslash
//! as inert, which covers `\"`, `\\`, `\/` and short escapes — but NOT
//! `\uXXXX`. A quote smuggled in as `\u0022` would desynchronize string
//! parity; extraction then safely fails to `None` (turn rejected, never a
//! corrupt apply). Real `pi` envelopes emit ASCII `\"`, so exposure is nil.

/// Find the JSON object substring inside agent text. Returns `None` when no
/// plausible object exists.
pub fn extract_json_object(text: &str) -> Option<String> {
    // 1) Fence hunting, last match wins.
    let mut last_fence_range: Option<(usize, usize)> = None;
    let _bytes = text.as_bytes();
    let mut i = 0usize;
    while let Some(found) = find_needle(text, i, "```json") {
        let content_start = found + "```json".len();
        let Some(close) = find_needle(text, content_start, "```") else { break };
        last_fence_range = Some((content_start, close));
        i = close + 3;
    }
    if let Some((s, e)) = last_fence_range {
        let inner = text[s..e].trim();
        if let Some(obj) = balanced_object(inner) {
            return Some(obj.to_string());
        }
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
    let mut depth = 0i32;
    let mut in_string = false;
    let mut escape = false;
    for (idx, ch) in json[start..].char_indices() {
        match ch {
            // Second half of an escape pair: inert, and clears the flag.
            _ if escape => escape = false,
            '"' => in_string = !in_string,
            '\\' if in_string => escape = true,
            '{' if !in_string => depth += 1,
            '}' if !in_string => {
                depth -= 1;
                if depth == 0 {
                    let end = start + idx + 1;
                    // The remainder after the object may be trailing junk;
                    // ensure nothing but whitespace follows the balanced span
                    // *within this fragment* (caller trims fragments).
                    if json[end..].trim().is_empty() {
                        return Some(&json[start..end]);
                    }
                    // Nested trailing content: retry scanning deeper? The agent
                    // contract forbids tails; treat as no-valid-object.
                    return None;
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
}
