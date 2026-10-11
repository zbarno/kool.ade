use std::{
    env, fs,
    sync::{Mutex, MutexGuard},
};

/// Held for the ENTIRE body of any shielded test so sibling git spawns
/// observe a consistent hierarchy, and so our mutation cannot bleed into
/// any sibling. Local repo configs are unaffected.
static GIT_HIERARCHY_LOCK: Mutex<()> = Mutex::new(());

/// Isolate global and system Git config layers for a test.
pub fn shield(tag: &str) -> Guard {
    shield_with_global_config(tag, "")
}

/// Isolate Git config while supplying a synthetic committer identity for tests
/// that exercise commands which write reflogs.
pub fn shield_with_synthetic_identity(tag: &str) -> Guard {
    shield_with_global_config(
        tag,
        "[user]\n\tname = Koolade Test Operator\n\temail = koolade-test@example.invalid\n",
    )
}

fn shield_with_global_config(tag: &str, contents: &str) -> Guard {
    let lock = GIT_HIERARCHY_LOCK.lock().unwrap_or_else(|p| p.into_inner());
    let dir = env::temp_dir().join(format!("koolade_git_shield_{tag}_{}", std::process::id()));
    fs::create_dir_all(&dir).unwrap();
    let global = dir.join("control.global.empty");
    let system = dir.join("control.system.empty");
    fs::write(&global, contents).unwrap();
    fs::write(&system, "").unwrap();
    // SAFETY: guarded by GIT_HIERARCHY_LOCK; no sibling Git test depends on
    // the ambient configuration while this guard is held.
    unsafe {
        env::set_var("GIT_CONFIG_GLOBAL", &global);
        env::set_var("GIT_CONFIG_SYSTEM", &system);
    }
    Guard {
        _lock: lock,
        _cleanup: dir,
    }
}

pub(crate) struct Guard {
    _lock: MutexGuard<'static, ()>,
    _cleanup: std::path::PathBuf,
}

impl Drop for Guard {
    fn drop(&mut self) {
        // SAFETY: same lock-guarded invariant as set in `shield_with_global_config`.
        unsafe {
            env::remove_var("GIT_CONFIG_GLOBAL");
            env::remove_var("GIT_CONFIG_SYSTEM");
        }
        let _ = fs::remove_dir_all(&self._cleanup);
    }
}
