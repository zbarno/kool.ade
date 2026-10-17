use super::stable_requirement_id;

#[test]
fn empty_req_prefix_falls_back_to_deterministic_generated_identity() {
    let first = stable_requirement_id("REQ-: preserve ownership");
    let repeated = stable_requirement_id("REQ-: preserve ownership");
    assert_ne!(first, "REQ-");
    assert!(first.starts_with("REQ-"));
    assert_eq!(first, repeated);
}
