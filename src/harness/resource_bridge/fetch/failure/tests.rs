use super::*;

#[test]
fn transferred_bytes_survive_error_context() {
    let error =
        with_bytes(42, anyhow::anyhow!("retrieval stopped")).context("fetching registry package");

    assert_eq!(bytes_from_failure(&error), Some(42));
}
