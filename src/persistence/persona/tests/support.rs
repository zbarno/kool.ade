use crate::persistence::persona::persona_path;
use std::{env, fs, path::PathBuf};

/// Hold `KOOLADE_HOME` pointed at a fresh pid-tagged temp dir. The shared
/// guard serializes only tests that mutate process-wide environment state.
pub(crate) struct TmpHome {
    pub(crate) home: PathBuf,
    _environment: crate::test_support::ProcessEnvironmentGuard,
}

impl TmpHome {
    pub(crate) fn new(tag: &str) -> Self {
        let home = env::temp_dir().join(format!("koolade_personas_{tag}_{}", std::process::id()));
        let _ = fs::remove_dir_all(&home);
        let mut environment = crate::test_support::ProcessEnvironmentGuard::new();
        environment.set("KOOLADE_HOME", &home);
        Self {
            home,
            _environment: environment,
        }
    }
}

impl Drop for TmpHome {
    fn drop(&mut self) {
        // Reverse permission changes before removing the fixture directory.
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let _ = fs::set_permissions(&self.home, fs::Permissions::from_mode(0o700));
        }
        let _ = fs::remove_dir_all(&self.home);
    }
}

pub(crate) fn persona_bytes() -> Vec<u8> {
    fs::read(persona_path()).expect("persona.md should exist on disk")
}
