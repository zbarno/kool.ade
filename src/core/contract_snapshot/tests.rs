use super::references_module;

#[test]
fn numeric_product_module_references_match_manifest_ids() {
    assert!(references_module(
        "Affected Product Areas: Module 05 (functional requirements)",
        "05-functional-requirements",
        "05-functional-requirements.md",
        "5. Functional Requirements",
    ));
    assert!(references_module(
        "Change affects module 5.",
        "05-functional-requirements",
        "05-functional-requirements.md",
        "5. Functional Requirements",
    ));
}

#[test]
fn numeric_module_references_keep_boundaries_and_stable_ids() {
    assert!(!references_module(
        "Affected Product Areas: Module 050",
        "05-functional-requirements",
        "05-functional-requirements.md",
        "5. Functional Requirements",
    ));
    assert!(references_module(
        "Affected Product Areas: product:05-functional-requirements",
        "05-functional-requirements",
        "05-functional-requirements.md",
        "5. Functional Requirements",
    ));
}
