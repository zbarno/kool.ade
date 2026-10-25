use super::*;

const MOD: &str =
    "module example.invalid/service\n\ngo 1.23\n\nrequire github.com/google/uuid v1.6.0\n";
const SUM: &str = "github.com/google/uuid v1.6.0 h1:NIvaJDMOsjHA8n1jAhLSgzrAzy1Hgr+hNrb57e+94F0=\ngithub.com/google/uuid v1.6.0/go.mod h1:TIyPZe4MgqvfeYDBFedMoGGpEw/LqOeaOT+nhxU+yHo=\n";

#[test]
fn accepts_exact_module_and_h1_sum_pair() {
    assert_eq!(
        manifest_inputs_from_bytes(MOD.as_bytes(), SUM.as_bytes())
            .unwrap()
            .len(),
        2
    );
    let packages = package_identities_from_contents(SUM).unwrap();
    assert_eq!(packages.len(), 2);
    assert_eq!(packages[0].source, PROXY_SOURCE);
}

#[test]
fn rejects_replacements_workspaces_non_exact_versions_and_malformed_sums() {
    let invalid = vec![
        MOD.replace("require github", "replace github"),
        MOD.replace("v1.6.0", "v1.6.0+local"),
        MOD.replace(
            "require github.com/google/uuid v1.6.0",
            "require github.com/google/uuid latest",
        ),
        format!("{MOD}\nexclude github.com/google/uuid v1.6.0\n"),
    ];
    for mod_text in invalid {
        assert!(parse_manifest(&mod_text).is_err());
    }
    assert!(parse_sums("github.com/google/uuid v1.6.0 h2:abc\n").is_err());
    assert!(
        manifest_inputs_from_bytes(MOD.as_bytes(), SUM.lines().next().unwrap().as_bytes()).is_err()
    );
}

#[test]
fn rejects_workspace_markers_and_symlinked_lock_inputs() {
    let root = tempfile::tempdir().unwrap();
    fs::write(root.path().join("go.mod"), MOD).unwrap();
    fs::write(root.path().join("go.sum"), SUM).unwrap();
    fs::write(root.path().join("go.work"), "go 1.23\n").unwrap();
    assert!(collect(root.path()).is_err());
    fs::remove_file(root.path().join("go.work")).unwrap();
    fs::remove_file(root.path().join("go.sum")).unwrap();
    #[cfg(unix)]
    std::os::unix::fs::symlink("go.mod", root.path().join("go.sum")).unwrap();
    #[cfg(unix)]
    assert!(collect(root.path()).is_err());
}
