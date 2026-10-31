//! Production planning request boundary: a long Unicode message must reach the
//! harness without exceeding the catalog's byte limit. A clipping unit test
//! cannot prove the request builder uses the byte budget. Uses a mock harness
//! and local Git; no provider, network, Docker, or SSH. Setup costs one temporary
//! repository and one local worker thread.
use super::*;

#[test]
fn long_unicode_planning_message_reaches_the_harness() {
    let message = "Clarify 質問🦀. ".repeat(2_000);
    let (inputs, dir) = inputs_for("unicode_prompt_bounds", &message);
    let sink = Arc::new(std::sync::Mutex::new(None));
    let controller = TurnController::start_read_only(
        inputs,
        Box::new(CaptureHarness {
            raw: canned_raw("Which behavior should the card display?"),
            sink: sink.clone(),
        }),
        None,
    );
    let outcome = drain(&controller);
    assert!(
        matches!(outcome, TurnOutcome::Discussion { .. }),
        "{outcome:?}"
    );
    let captured = sink.lock().unwrap();
    let prompt = captured.as_ref().expect("planning harness was reached");
    let key = "\"original_goal\":";
    let start = prompt.find(key).expect("catalog common context") + key.len();
    let mut decoder = serde_json::Deserializer::from_str(prompt[start..].trim_start());
    let goal = <String as serde::Deserialize>::deserialize(&mut decoder).unwrap();
    assert!(goal.len() <= 16 * 1024);
    assert!(goal.contains("Clarify 質問🦀."));
    assert!(goal.ends_with("\n[…truncated…]"));
    drop(captured);
    let _ = std::fs::remove_dir_all(dir);
}
