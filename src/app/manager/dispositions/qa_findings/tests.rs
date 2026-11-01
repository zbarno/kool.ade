use super::parse_work_key;

#[test]
fn qa_repair_work_key_binds_exact_run_and_finding_ids() {
    let run = "00000000-0000-4000-8000-000000000001";
    let finding = "00000000-0000-4000-8000-000000000002";
    assert_eq!(
        parse_work_key(&format!("task:runtime-qa:{run}:{finding}")),
        Some((run, finding))
    );
    assert!(parse_work_key("task:runtime-qa:run:finding").is_none());
    assert!(parse_work_key(&format!("task:runtime-qa:{run}:{finding}:extra")).is_none());
}
