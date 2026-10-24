#[path = "support/fixture.rs"]
mod fixture;
#[path = "support/receipts.rs"]
mod receipts;
pub(super) use fixture::decomposer_graph;
pub(crate) use fixture::setup;
pub(crate) use receipts::{
    assert_current_test_plan_confirmation, record_test_plan_author, record_test_plan_review,
};

pub(crate) fn remove_fixture_root(root: &std::path::Path) {
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(2);
    loop {
        match std::fs::remove_dir_all(root) {
            Ok(()) => return,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => return,
            Err(error)
                if error.kind() == std::io::ErrorKind::DirectoryNotEmpty
                    && std::time::Instant::now() < deadline =>
            {
                std::thread::sleep(std::time::Duration::from_millis(10));
            }
            Err(error) => panic!("failed to remove test fixture {}: {error}", root.display()),
        }
    }
}
