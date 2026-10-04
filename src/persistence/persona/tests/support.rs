use crate::persistence::persona::persona_path;
use std::{env, fs, path::PathBuf};

// KOOLADE_HOME is process-global state: whichever test flips it must hold
// this for its WHOLE body, or siblings' reads land on the wrong home.
// Sibling env tests (chat_store) flip the same variable under their OWN
// lock, so in-module serialization alone cannot exclude them; the settle
// gate below additionally arranges that our claim never straddles a
// neighbor's (microsecond-scale) body, and the drop tripwire certifies
// the partition held for the entire critical section. The guard also
// tears the temp home down (restoring writability first), so
// parallel-suite siblings always see a writable world.
static ENV_HOME_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

/// Settle-gate cadence: one quiet wait far exceeds any neighboring env-
/// test body (microseconds of file IO); later rounds escalate the wait
/// when a sample pair straddles a live transition.
const SETTLE_WAITS_MS: [u64; 5] = [50, 100, 200, 400, 400];
const SAMPLE_GAP_MICROS: u64 = 100;

/// Hold `KOOLADE_HOME` pointed at a fresh pid-tagged temp dir for the
/// guard's lifetime; on drop, certify the variable was undisturbed,
/// restore permissions, and remove the dir.
pub(crate) struct TmpHome {
    pub(crate) home: PathBuf,
    _lock: std::sync::MutexGuard<'static, ()>,
}

impl TmpHome {
    pub(crate) fn new(tag: &str) -> Self {
        let lock = ENV_HOME_LOCK
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        let home = env::temp_dir().join(format!("koolade_personas_{tag}_{}", std::process::id()));

        // Settle gate: claim only when KOOLADE_HOME is observably
        // stationary, so a neighbor env test in flight cannot straddle
        // our claim. Escalate per round; after exhaustion fail loudly
        // rather than wedge the suite (drops the lock before panicking).
        for wait_ms in SETTLE_WAITS_MS {
            std::thread::sleep(std::time::Duration::from_millis(wait_ms));
            let before = env::var_os("KOOLADE_HOME");
            std::thread::sleep(std::time::Duration::from_micros(SAMPLE_GAP_MICROS));
            if before != env::var_os("KOOLADE_HOME") {
                continue; // a live transition crossed the sample pair
            }
            let _ = fs::remove_dir_all(&home);
            // SAFETY: guarded by ENV_HOME_LOCK; the settle gate rules
            // out concurrent KOOLADE_HOME transitions at claim time.
            unsafe { env::set_var("KOOLADE_HOME", &home) };
            assert_eq!(
                env::var_os("KOOLADE_HOME").as_deref(),
                Some(home.as_os_str()),
                "KOOLADE_HOME was overwritten between claim and verify",
            );
            return TmpHome { home, _lock: lock };
        }
        drop(lock);
        panic!("could not observe a settled KOOLADE_HOME in five escalated waits");
    }
}

impl Drop for TmpHome {
    fn drop(&mut self) {
        // Tripwire: a variable disturbed inside the critical section
        // voids this test's filesystem observations. The settle gate
        // should make this unreachable, so a tripped wire demands
        // attention instead of swallowing the race as a pass-or-fail.
        assert_eq!(
            env::var_os("KOOLADE_HOME").as_deref(),
            Some(self.home.as_os_str()),
            "KOOLADE_HOME was disturbed during the persona critical section",
        );
        // Reverse any permission mischief before removal (removing a
        // read-only dir's children would otherwise fail), then wipe it.
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
