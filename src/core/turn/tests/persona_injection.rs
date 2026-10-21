use super::*;
use crate::persistence::persona::{SHIPPED_DEFAULT_PERSONA, persona_path, save_persona};

/// The layer's header line, at the very start of the intro, so
/// locating it locates the layer.
/// Multi-line sentinels, DELIBERATELY without a trailing newline:
/// the composition must not tack one on.
const SENTINEL_A: &str =
    "# Sentinel A: tuned voice\n- Terse, warm, decisive\n- Ask one question at a time";
const SENTINEL_B: &str =
    "# Sentinel B: retuned voice\n- Warmer opener\n- Still terse\n- Flag ambiguity eagerly";

const SETTLE_WAITS_MS: [u64; 5] = [50, 100, 200, 400, 400];
const SAMPLE_GAP_MICROS: u64 = 100;
/// Full-redo budget per leg when a neighbor's env flip crosses
/// the window despite the settle gate.
const MAX_LEG_ATTEMPTS: u32 = 5;

/// Verdict of one leg attempt: `Bound` — ran and its binding
/// assertions held; `Crossing` — its own assertions indicate the
/// turn did NOT bind to our temp home (suspected neighbor env
/// flip; redo); `Fatal(msg)` — a genuine failure, independent of
/// env.
enum LegResult {
    Bound,
    Crossing,
    Fatal(String),
}

/// Runs the leg under bounded full redos until it binds cleanly.
/// The leg OWNS its `LockedPersonaHome` (unique per attempt) and
/// pushes human-readable observations to `obs` whenever it sees
/// something amiss; the budget-exhausted panic prints them.
fn run_leg(mut leg: impl FnMut(u32, &mut Vec<String>) -> LegResult) {
    let mut obs = Vec::new();
    let mut attempt = 0u32;
    loop {
        attempt += 1;
        match leg(attempt, &mut obs) {
            LegResult::Bound => return,
            LegResult::Fatal(reason) => panic!(
                "persona leg failed on attempt {attempt}: {reason}\nobservations:\n{}",
                obs.join("\n")
            ),
            LegResult::Crossing if attempt < MAX_LEG_ATTEMPTS => {
                obs.push(format!(
                    "attempt {attempt} observed a foreign home; redoing"
                ));
            }
            LegResult::Crossing => panic!(
                "persona leg failed to bind to its own temp home across {attempt} attempts (sustained env interference)\nobservations:\n{}",
                obs.join("\n")
            ),
        }
    }
}

/// True while `KOOLADE_HOME` still names our claimed home — used as
/// a pre-flight before spinning a turn AND as the post-Done
/// crossing check: a neighbor's mid-turn steal of the variable
/// means our turn's persistence may have written into their home,
/// so the leg must redo rather than release the claim dirty.
fn bound_here(home: &LockedPersonaHome) -> bool {
    std::env::var_os("KOOLADE_HOME").as_deref() == Some(home.home.as_os_str())
}

/// Holds `KOOLADE_HOME` pointed at a fresh tagged temp dir for the
/// guard's lifetime; on drop it REMOVES the variable (not
/// restores) and wipes the dir, both under the still-held lock.
struct LockedPersonaHome {
    home: std::path::PathBuf,
    _lock: crate::test_support::ProcessEnvironmentLock,
}

impl LockedPersonaHome {
    fn new(tag: &str) -> Self {
        let lock = crate::test_support::lock_process_environment();
        let home = std::env::temp_dir().join(format!(
            "koolade_persona_inject_{tag}_{}",
            uuid::Uuid::new_v4()
        ));
        for wait_ms in SETTLE_WAITS_MS {
            std::thread::sleep(std::time::Duration::from_millis(wait_ms));
            let before = std::env::var_os("KOOLADE_HOME");
            std::thread::sleep(std::time::Duration::from_micros(SAMPLE_GAP_MICROS));
            if before != std::env::var_os("KOOLADE_HOME") {
                continue; // a live transition crossed the sample pair
            }
            std::fs::create_dir_all(&home).unwrap();
            // SAFETY: guarded by the shared process environment lock.
            unsafe { std::env::set_var("KOOLADE_HOME", &home) };
            assert_eq!(
                std::env::var_os("KOOLADE_HOME").as_deref(),
                Some(home.as_os_str()),
                "KOOLADE_HOME was overwritten between claim and verify",
            );
            return LockedPersonaHome { home, _lock: lock };
        }
        drop(lock);
        panic!("could not observe a settled KOOLADE_HOME in five escalated waits");
    }
}

impl Drop for LockedPersonaHome {
    fn drop(&mut self) {
        // Panic-free by construction — this may run during unwind.
        unsafe { std::env::remove_var("KOOLADE_HOME") };
        let _ = std::fs::remove_dir_all(&self.home);
    }
}

/// Sibling drain collector (incumbent helpers stay unmodified):
/// drains the controller to Done, collecting every LiveProgress
/// activity string observed along the way. Scripted harness runs
/// send no real progress, so anything collected here is the
/// pre-execution diagnostic notice. Incumbent-paced 250ms polls:
/// gitops subprocess launches open LiveProgress gaps in the tens
/// of ms, so the probe interval must exceed them. A bounded terminal deadline
/// distinguishes ordinary silence from a truly vanished turn (closed channel
/// idles forever) without assuming the worker will run within two seconds.
fn drain_collecting(controller: &TurnController) -> (TurnOutcome, Vec<String>) {
    const TERMINAL_TIMEOUT: Duration = Duration::from_secs(30);
    let mut activities = Vec::new();
    let deadline = std::time::Instant::now() + TERMINAL_TIMEOUT;
    loop {
        let remaining = deadline.saturating_duration_since(std::time::Instant::now());
        if remaining.is_zero() {
            panic!("turn vanished: no terminal event within {TERMINAL_TIMEOUT:?}");
        }
        match controller.poll(Duration::from_millis(250).min(remaining)) {
            Some(TurnEvt::Progress(p)) => {
                if let Some(activity) = p.activity {
                    activities.push(activity);
                }
            }
            Some(TurnEvt::Done(o)) => return (*o, activities),
            None => {}
        }
    }
}

fn controller_for(
    inputs: TurnInputs,
    sink: &Arc<std::sync::Mutex<Option<String>>>,
    script: &str,
) -> (TurnController, Arc<std::sync::Mutex<Option<String>>>) {
    (
        TurnController::start(
            inputs,
            Box::new(CaptureHarness {
                raw: canned_raw(script),
                sink: Arc::clone(sink),
            }),
        ),
        Arc::clone(sink),
    )
}

#[path = "persona_injection_tests/fallback.rs"]
mod fallback;
#[path = "persona_injection_tests/resaved_sentinel.rs"]
mod resaved_sentinel;
#[path = "persona_injection_tests/saved_sentinel.rs"]
mod saved_sentinel;
