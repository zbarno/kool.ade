use std::env;

#[path = "src/build_identity/git.rs"]
mod git;

fn main() {
    println!("cargo:rerun-if-env-changed=KOOLADE_BUILD_COMMIT");
    println!("cargo:rerun-if-changed=src");
    for path in git::rerun_paths(".") {
        println!("cargo:rerun-if-changed={path}");
    }
    let configured = env::var("KOOLADE_BUILD_COMMIT").ok();
    let clean = git::worktree_is_clean(".");
    let detected = clean.then(|| git::head_commit(".")).flatten();
    let commit = detected
        .filter(|value| git::valid_commit(value))
        .or_else(|| {
            clean
                .then(|| configured.filter(|value| git::valid_commit(value)))
                .flatten()
        })
        .unwrap_or_else(|| "unknown".to_owned());
    println!("cargo:rustc-env=KOOLADE_BUILD_COMMIT={commit}");
}
