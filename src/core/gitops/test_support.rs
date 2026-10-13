use std::{env, fs};

/// Held for the ENTIRE body of any shielded test so sibling git spawns
/// observe a consistent hierarchy, and so our mutation cannot bleed into
/// any sibling. Local repo configs are unaffected.
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
    let mut environment = crate::test_support::ProcessEnvironmentGuard::new();
    let dir = env::temp_dir().join(format!("koolade_git_shield_{tag}_{}", std::process::id()));
    fs::create_dir_all(&dir).unwrap();
    let global = dir.join("control.global.empty");
    let system = dir.join("control.system.empty");
    fs::write(&global, contents).unwrap();
    fs::write(&system, "").unwrap();
    environment.set("GIT_CONFIG_GLOBAL", &global);
    environment.set("GIT_CONFIG_SYSTEM", &system);
    // Command-line config injections override the isolated global/system files.
    // Clear them for the duration of the shield and restore ambient values on drop.
    environment.remove("GIT_CONFIG_COUNT");
    environment.remove("GIT_CONFIG_PARAMETERS");
    Guard {
        _environment: environment,
        _cleanup: dir,
    }
}

pub(crate) struct Guard {
    _environment: crate::test_support::ProcessEnvironmentGuard,
    _cleanup: std::path::PathBuf,
}

impl Drop for Guard {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self._cleanup);
    }
}
