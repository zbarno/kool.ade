use super::super::*;
use super::helpers::*;
use crate::core::gitops;
#[test]
fn perform_clone_local_source_lands_cleanly_behind_the_planning_setup_gate() {
    // Chain proof under the enforced model: perform_clone delivers a
    // healthy working tree (destination layout, files, scratch hygiene),
    // and the connect step it feeds into REFUSES a planning-less source
    // with actionable setup guidance — a bare clone acquires zero
    // planning state on its own.
    let sb = EnvSandbox::enter("chain", false);
    let src = local_source_repo("chain"); // no planning/ — cold start

    let dest =
        perform_clone(src.to_str().unwrap(), "sw-clone").expect("offline local-source clone lands");
    assert_eq!(
        dest,
        sb.home.join("kool-ade-workspaces/sw-clone"),
        "$HOME/kool-ade-workspaces joined with the repo segment verbatim"
    );
    assert!(gitops::is_work_tree(&dest), "the clone is a working tree");
    assert!(dest.join("README.md").exists(), "files arrived");
    no_scratch_left(&sb.home);

    match attempt_connect(dest.to_str().unwrap()) {
        Ok(project) => {
            panic!(
                "the planning-less clone must not connect ({:?})",
                project.state.title
            )
        }
        Err(err) => {
            let msg = format!("{} | {}", err.headline(), err.detail());
            assert!(
                msg.contains("planning repository"),
                "refusal carries actionable planning-setup guidance (got: {msg})"
            );
        }
    }
    assert!(
        !dest.join(".koolade-packet").exists(),
        "the refusal planted no planning state in the clone"
    );
    let _ = std::fs::remove_dir_all(&src);
}
