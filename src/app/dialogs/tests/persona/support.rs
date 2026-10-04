use std::time::Duration;
// ---- Environment discipline (env-touching test ONLY) ---------------
//
// KOOLADE_HOME is PROCESS-GLOBAL, and the other env-touching suites
// (persona, chat_store) hold THEIR OWN locks — so nothing external
// serializes us against THEM. A naive claim could therefore be straddled
// by a neighbour's microsecond env flip: our file reads would land in
// their home, their tripwires would fire on our value, and the resulting
// destructor panic aborts the whole run. Defence here is layered:
//   1. ONE short-lived claim for the ENTIRE disk-effect journey (minimal
//      exposed surface), reusing story 001's settle gate at claim time;
//   2. setup-theft detection (our value overwritten between set-var and
//      verify ⇒ abandon + retry, never panic at claim time);
//   3. a CHECKPOINT at every phase boundary detecting mid-journey flips;
//   4. a bounded RETRY LOOP turning a rare race into noise-free flake
//      absorption instead of a red run;
//   5. silent, exact pre-claim-value restoration on drop (a panic inside
//      a destructor would non-unwind-abort the process — forbidden).
static PERSONA_ENV_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

const CLAIM_SETTLE_WAITS_MS: [u64; 4] = [20, 50, 120, 240];
const CLAIM_SAMPLE_GAP_MICROS: u64 = 100;
pub(super) const JOURNEY_ATTEMPTS: u32 = 3;

/// Lost the race to (or was raced by) a neighbouring env test.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum Contested {
    Unsettled,
    StolenAtSetup,
    FlippedMidJourney,
}

pub(super) struct EnvClaim {
    prior: Option<std::ffi::OsString>,
    parent: std::path::PathBuf,
    last_home: std::path::PathBuf,
    _lock: std::sync::MutexGuard<'static, ()>,
}

impl EnvClaim {
    pub(super) fn begin(tag: &str) -> Result<Self, Contested> {
        let _lock = match PERSONA_ENV_LOCK.lock() {
            Ok(guard) => guard,
            Err(poisoned) => poisoned.into_inner(),
        };
        let mut settled = false;
        for wait_ms in CLAIM_SETTLE_WAITS_MS {
            std::thread::sleep(Duration::from_millis(wait_ms));
            let before = std::env::var_os("KOOLADE_HOME");
            std::thread::sleep(Duration::from_micros(CLAIM_SAMPLE_GAP_MICROS));
            if before == std::env::var_os("KOOLADE_HOME") {
                settled = true;
                break;
            }
        }
        if !settled {
            drop(_lock);
            return Err(Contested::Unsettled);
        }
        let prior = std::env::var_os("KOOLADE_HOME");
        let parent = std::env::temp_dir().join(format!(
            "koolade_dialogs_persona_{tag}_{}",
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&parent);
        std::fs::create_dir_all(&parent).expect("phase parent must be creatable");
        let home = parent.join("claim");
        std::fs::create_dir_all(&home).unwrap();
        // SAFETY: PERSONA_ENV_LOCK held; the settle gate ruled out an
        // in-flight KOOLADE_HOME transition at claim time.
        unsafe { std::env::set_var("KOOLADE_HOME", &home) };
        if std::env::var_os("KOOLADE_HOME").as_deref() != Some(home.as_os_str()) {
            // A neighbour snuck a flip into the claim handshake: undo
            // ours, restore theirs-visible prior, and let the caller
            // retry — NEVER panic while holding the claim.
            Self::restore_env(prior.as_ref());
            let _ = std::fs::remove_dir_all(&parent);
            return Err(Contested::StolenAtSetup);
        }
        Ok(Self {
            prior,
            parent,
            last_home: home,
            _lock,
        })
    }

    /// Phase-boundary heartbeat: our value still owned?
    pub(super) fn checkpoint(&self) -> Result<(), Contested> {
        if std::env::var_os("KOOLADE_HOME").as_deref() == Some(self.last_home.as_os_str()) {
            Ok(())
        } else {
            Err(Contested::FlippedMidJourney)
        }
    }

    /// Point the claim at a fresh sibling child dir (one phase home).
    pub(super) fn new_phase(&mut self, tag: &str) -> std::path::PathBuf {
        let dir = self.parent.join(tag);
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        // SAFETY: still under the held PERSONA_ENV_LOCK.
        unsafe { std::env::set_var("KOOLADE_HOME", &dir) };
        self.last_home = dir.clone();
        dir
    }

    fn restore_env(prior: Option<&std::ffi::OsString>) {
        // SAFETY: called with the PERSONA_ENV_LOCK held (setup failure
        // path) or by Drop (the guard is still alive in both).
        match prior {
            Some(previous) => unsafe { std::env::set_var("KOOLADE_HOME", previous) },
            None => unsafe { std::env::remove_var("KOOLADE_HOME") },
        }
    }
}

impl Drop for EnvClaim {
    fn drop(&mut self) {
        // Silent, exact, UNCONDITIONAL restoration of whatever preceded
        // the claim. Panics are forbidden here: a panic during cleanup
        // aborts the whole test process.
        Self::restore_env(self.prior.as_ref());
        let _ = std::fs::remove_dir_all(&self.parent);
    }
}
