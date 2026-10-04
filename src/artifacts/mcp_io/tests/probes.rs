use super::*;

/// Positive table: ANY syntactically valid JSON passes, schema blind.
/// Top-level `5` is legal JSON and MUST pass — the consumer's schema
/// is outside the planner (D-16).
#[test]
fn probe_accepts_objects_arrays_scalars_and_unicode_keys() {
    for sample in [
        "{\"mcpServers\": {\"x\": {\"command\": \"true\"}}}",
        "[1, 2, 3]",
        "5",
        "\"quoted\"",
        "null",
        "{\"uniçødé κey\": {\"в\": true}}",
    ] {
        assert_eq!(probe_json(sample), Ok(()), "sample: {sample}");
    }
}

/// Negative table: every entry is ill-formed syntax and yields a
/// NON-EMPTY human message (the dialog quotes it verbatim).
#[test]
fn probe_rejects_truncation_trailing_comma_prose_and_double_docs() {
    for sample in [
        "{\"servers\":",        // truncation
        "{ \"a\": 1, }",        // trailing comma
        "not json",             // plain prose
        "{\"a\":1}\n{\"b\":2}", // double document
    ] {
        let err = probe_json(sample)
            .err()
            .unwrap_or_else(|| panic!("expected Err for: {sample}"));
        assert!(
            !err.trim().is_empty(),
            "probe message must be non-empty for: {sample}"
        );
    }
}
