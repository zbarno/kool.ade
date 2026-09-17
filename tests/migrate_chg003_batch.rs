//! One-shot data repair (September 2026): the CHG-003 task batch was published
//! while the active feature was still CHG-002, freezing CHG-002's identity,
//! specification and product-module selection into `contract.json`. The story
//! `Feature ID` lines and the batch `specification.md` were corrected by hand
//! (plain text); this test rewrites ONLY `contract.json` by running the
//! application's own `contract_snapshot::freeze()` so the repaired batch is
//! byte-compatible with app-generated contracts, including the canonical
//! configuration serialization.
//!
//! Gated twice on purpose: `#[ignore]` (normal suites never run it) and the
//! `RUN_MIGRATE_CHG003_BATCH=1` environment variable. Refuses to operate when
//! the active feature is not CHG-003, so it cannot corrupt a future batch.
//! Once the migration has been applied and committed, this file may simply be
//! deleted; nothing else references it.

#[cfg(test)]
mod migration {
    use packet::{core::contract_snapshot, core::state::PlannerState};

    fn repo_root() -> std::path::PathBuf {
        std::path::Path::new(env!("CARGO_MANIFEST_DIR")).to_path_buf()
    }

    #[test]
    #[ignore = "one-shot CHG-003 batch repair; opt in with RUN_MIGRATE_CHG003_BATCH=1"]
    fn restore_chg003_batch_contract() {
        assert_eq!(
            std::env::var_os("RUN_MIGRATE_CHG003_BATCH").as_deref(),
            Some(std::ffi::OsStr::new("1")),
            "explicit operator consent required"
        );
        let root = repo_root();
        let state = PlannerState::load(&root).expect("load planning state");
        let snapshot = contract_snapshot::freeze(&state)
            .expect("freeze contract")
            .expect("contract present for the active feature");
        assert_eq!(
            snapshot.feature_id, "CHG-003",
            "active feature drifted away from CHG-003; aborting so no future batch can be corrupted"
        );
        let body = state
            .active_feature
            .as_ref()
            .expect("CHG-003 must be the active feature")
            .1
            .clone();
        // Belt and braces: the frozen specification must be the CHG-003 document.
        assert!(
            body.starts_with("# CHG-003:"),
            "unexpected feature document head: {}\n...",
            body.lines().next().unwrap_or("<empty>")
        );
        let batch = root.join(
            "planning/tasks/readable-chat-replies-with-at-a-glance-asks-and-quick-op",
        );
        let spec_path = batch.join("specification.md");
        assert!(
            body == std::fs::read_to_string(&spec_path).expect("batch specification.md"),
            "fix the batch specification.md to the current CHG-003 feature document first"
        );
        let path = batch.join("contract.json");
        let payload =
            serde_json::to_string_pretty(&snapshot).expect("serialize contract");
        std::fs::write(&path, format!("{payload}\n")).expect("write contract.json");
        println!("rewrote {path:?} for feature {}", snapshot.feature_id);
        println!(
            "product modules: {:?}",
            snapshot.product_modules.keys().collect::<Vec<_>>()
        );
        println!("migration complete; commit the diff, then this file may be deleted");
    }
}
