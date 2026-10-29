#[path = "polyglot/fixture.rs"]
mod fixture;
#[path = "polyglot/runtime.rs"]
mod runtime;

/// Production boundary: discover the checked-in TypeScript/Python monorepo,
/// approve and explicitly select each nested target, then run each target's
/// Build/Test/Start recipes through its locked rootless image. The Node build
/// must read its task-scoped broker cache while network access is denied. Feed
/// both receipt sets through production request/artifact preparation and the
/// browser driver. Unit tests cannot prove the Docker workspace root,
/// image/tool resolution, broker cache mount, receipt provenance, and task
/// preview together. Dependencies: rootless Docker plus locked Node, Python,
/// and browser QA images. Setup cost: six short recipes and two browser runs.
#[test]
#[ignore = "requires rootless Docker and the locked Node/Python executor images"]
fn rootless_polyglot_targets_run_approved_recipes_with_exact_receipts() {
    let fixture = fixture::Fixture::new();
    let dependency_request_id = fixture.dependency_cache.request_id.clone();
    runtime::run(fixture, dependency_request_id);
}
