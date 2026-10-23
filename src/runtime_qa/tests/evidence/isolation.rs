use super::report_fixture;
use crate::runtime_qa::RuntimeQaStore;

#[test]
// Production boundary: parallel app-owned QA workers persist different run
// IDs below one workspace evidence root. Sequential unit tests cannot expose
// cross-run path collisions; this test uses only temporary files and threads,
// with no Docker, network, SSH, or credentials and low setup cost.
fn parallel_qa_runs_keep_requests_reports_and_screenshots_isolated() {
    let directory = tempfile::tempdir().unwrap();
    let root = directory.path().join("qa");
    let store = RuntimeQaStore::new(&root);
    let left = report_fixture();
    let right = report_fixture();
    assert_ne!(left.request.qa_run_id, right.request.qa_run_id);
    let left_id = left.request.qa_run_id.clone();
    let right_id = right.request.qa_run_id.clone();

    let left_store = store.clone();
    let left_write = std::thread::spawn(move || {
        left_store.save_run_with_screenshots(
            &left.request,
            &left.scenarios,
            &left.evidence,
            &left.result,
            std::slice::from_ref(&left.screenshot),
        )
    });
    let right_write = std::thread::spawn(move || {
        store.save_run_with_screenshots(
            &right.request,
            &right.scenarios,
            &right.evidence,
            &right.result,
            std::slice::from_ref(&right.screenshot),
        )
    });
    left_write.join().unwrap().unwrap();
    right_write.join().unwrap().unwrap();

    let left_run = RuntimeQaStore::new(&root)
        .load_run_with_screenshots(&left_id)
        .unwrap();
    let right_run = RuntimeQaStore::new(&root)
        .load_run_with_screenshots(&right_id)
        .unwrap();
    assert_eq!(left_run.request.qa_run_id, left_id);
    assert_eq!(right_run.request.qa_run_id, right_id);
    assert_ne!(
        left_run.evidence.screenshots[0].storage_ref,
        right_run.evidence.screenshots[0].storage_ref
    );
    assert_eq!(left_run.screenshots.len(), 1);
    assert_eq!(right_run.screenshots.len(), 1);
}
