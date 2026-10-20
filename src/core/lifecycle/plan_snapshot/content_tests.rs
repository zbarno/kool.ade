use super::{acceptance_criteria_refs, requirement_rationale, stable_requirement_id};

#[test]
fn empty_req_prefix_falls_back_to_deterministic_generated_identity() {
    let first = stable_requirement_id("REQ-: preserve ownership");
    let repeated = stable_requirement_id("REQ-: preserve ownership");
    assert_ne!(first, "REQ-");
    assert!(first.starts_with("REQ-"));
    assert_eq!(first, repeated);
}

#[test]
fn requirement_context_captures_rationale_and_acceptance_links() {
    let text = "REQ-001: Keep reports exportable for AC-1 and AC2. Rationale: preserves the agreed workflow.";
    assert_eq!(requirement_rationale(text), "preserves the agreed workflow");
    assert_eq!(acceptance_criteria_refs(text), ["AC-1", "AC2"]);
    assert_eq!(
        requirement_rationale("REQ-2: Keep the existing control."),
        "Not specified in the source specification."
    );
}
