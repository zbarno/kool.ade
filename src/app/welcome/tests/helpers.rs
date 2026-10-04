use std::path::{Path, PathBuf};
pub(super) fn local_source_repo(tag: &str) -> PathBuf {
    let p = std::env::temp_dir().join(format!("koolade_swclone_src_{tag}_{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&p);
    std::fs::create_dir_all(&p).unwrap();
    let git = |args: &[&str]| {
        let status = std::process::Command::new("git")
            .arg("-C")
            .arg(&p)
            .args(args)
            .status()
            .unwrap();
        assert!(status.success(), "git {args:?} failed");
    };
    git(&["init", "-q", "-b", "main"]);
    git(&["config", "user.name", "Clone Src"]);
    git(&["config", "user.email", "src@example.invalid"]);
    std::fs::write(p.join("README.md"), "# Cloned source\n").unwrap();
    git(&["add", "README.md"]);
    git(&["commit", "-q", "-m", "initial"]);
    p
}

pub(super) fn no_scratch_left(home: &Path) {
    let offenders: Vec<_> = std::fs::read_dir(home.join("kool-ade-workspaces"))
        .unwrap()
        .flatten()
        .filter(|e| e.file_name().to_string_lossy().contains("koolade-cloning"))
        .collect();
    assert!(offenders.is_empty(), "scratch remnants: {offenders:?}");
}

/// Fresh throwaway home directory (NO env involvement at all —
/// [`super::perform_clone_at`] takes it by reference).
pub(super) fn scratch_home(tag: &str) -> PathBuf {
    let p = std::env::temp_dir().join(format!("koolade_swclone_home_{tag}_{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&p);
    std::fs::create_dir_all(&p).unwrap();
    p
}

// ---- perform_clone (OFFLINE: local file-path git sources) --------------
//
// Environment-mutation discipline (house convention, cf. gitops'
// test_support::shield): the HOUSE GIT_HIERARCHY_LOCK shield held for
// the WHOLE test body — every other env-mutating suite member (glue,
// feature_approval, root) takes the same lock, so windows never
// interleave; HOME and KOOLADE_HOME restored on drop.

pub(super) struct EnvSandbox {
    _shield: crate::core::gitops::test_support::Guard,
    prev_home: Option<std::ffi::OsString>,
    prev_koolade_home: Option<std::ffi::OsString>,
    pub(super) home: PathBuf,
}

impl EnvSandbox {
    pub(super) fn enter(tag: &str, drop_home: bool) -> Self {
        let _shield = crate::core::gitops::test_support::shield(tag);
        let home =
            std::env::temp_dir().join(format!("koolade_swclone_home_{tag}_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&home);
        std::fs::create_dir_all(&home).unwrap();
        let koolade_home = home.join("koolade-state");
        std::fs::create_dir_all(&koolade_home).unwrap();
        let prev_home = std::env::var_os("HOME");
        let prev_koolade_home = std::env::var_os("KOOLADE_HOME");
        // SAFETY: the house shield is held for this whole body; no
        // sibling test mutates or depends on HOME/KOOLADE_HOME while it
        // is held.
        unsafe {
            if drop_home {
                std::env::remove_var("HOME");
            } else {
                std::env::set_var("HOME", &home);
            }
            std::env::set_var("KOOLADE_HOME", &koolade_home);
        }
        Self {
            _shield,
            prev_home,
            prev_koolade_home,
            home,
        }
    }
}

impl Drop for EnvSandbox {
    fn drop(&mut self) {
        // SAFETY: the shield still guards (its field drops after this
        // body), so the restore cannot interleave with any sibling.
        unsafe {
            match &self.prev_home {
                Some(value) => std::env::set_var("HOME", value),
                None => std::env::remove_var("HOME"),
            }
            match &self.prev_koolade_home {
                Some(value) => std::env::set_var("KOOLADE_HOME", value),
                None => std::env::remove_var("KOOLADE_HOME"),
            }
        }
        let _ = std::fs::remove_dir_all(&self.home);
    }
}
