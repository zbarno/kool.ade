use super::*;
use crate::core::prompt::{PERSONA_LAYER_INTRO, PLANNER_POLICY};
use crate::persistence::persona::{SHIPPED_DEFAULT_PERSONA, persona_path, save_persona};

/// The layer's header line, at the very start of the intro, so
/// locating it locates the layer.
const PERSONA_MARKER: &str = "OPERATOR PERSONA (subordinate overlay)";
/// Multi-line sentinels, DELIBERATELY without a trailing newline:
/// the composition must not tack one on.
const SENTINEL_A: &str =
    "# Sentinel A: tuned voice\n- Terse, warm, decisive\n- Ask one question at a time";
const SENTINEL_B: &str =
    "# Sentinel B: retuned voice\n- Warmer opener\n- Still terse\n- Flag ambiguity eagerly";

/// Whole-test serializer: this module's legs never interleave
/// (nesting order is always SERIALIZER outside, HOME_LOCK
/// inside, everywhere).
static LEG_SERIALIZER: std::sync::Mutex<()> = std::sync::Mutex::new(());
static HOME_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());
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
    _lock: std::sync::MutexGuard<'static, ()>,
}

impl LockedPersonaHome {
    fn new(tag: &str) -> Self {
        let lock = HOME_LOCK
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        let home = std::env::temp_dir().join(format!(
            "koolade_persona_inject_{tag}_{}",
            std::process::id()
        ));
        for wait_ms in SETTLE_WAITS_MS {
            std::thread::sleep(std::time::Duration::from_millis(wait_ms));
            let before = std::env::var_os("KOOLADE_HOME");
            std::thread::sleep(std::time::Duration::from_micros(SAMPLE_GAP_MICROS));
            if before != std::env::var_os("KOOLADE_HOME") {
                continue; // a live transition crossed the sample pair
            }
            std::fs::create_dir_all(&home).unwrap();
            // SAFETY: guarded by HOME_LOCK; the settle gate rules
            // out concurrent KOOLADE_HOME transitions at claim time.
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
/// of ms, so the probe interval must exceed them. A bounded
/// run of consecutive idle probes distinguishes ordinary silence
/// from a truly vanished turn (closed channel idles forever).
fn drain_collecting(controller: &TurnController) -> (TurnOutcome, Vec<String>) {
    const IDLE_BUDGET: usize = 8;
    let mut activities = Vec::new();
    let mut idle = 0usize;
    loop {
        match controller.poll(Duration::from_millis(250)) {
            Some(TurnEvt::Progress(p)) => {
                idle = 0;
                if let Some(activity) = p.activity {
                    activities.push(activity);
                }
            }
            Some(TurnEvt::Done(o)) => return (*o, activities),
            None => {
                idle += 1;
                if idle >= IDLE_BUDGET {
                    panic!("turn vanished: no terminal event within {IDLE_BUDGET} idle probes");
                }
            }
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

#[test]
fn saved_sentinel_rides_both_conversation_modes_from_the_next_turn_on() {
    let _serial = LEG_SERIALIZER
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());

    // Main-Chat leg.
    run_leg(|attempt, obs| {
        let (inputs, dir_main) = inputs_for("pj_main", "Speak in my tuned voice.");
        let home = LockedPersonaHome::new(&format!("modes_main_{attempt}"));
        save_persona(SENTINEL_A).expect("sentinel save must succeed");
        if !bound_here(&home) {
            obs.push(format!(
                "main leg: pre-flight env check lost at attempt {attempt}"
            ));
            let _ = std::fs::remove_dir_all(&dir_main);
            return LegResult::Crossing;
        }
        let sink: Arc<std::sync::Mutex<Option<String>>> = Arc::new(std::sync::Mutex::new(None));
        let (c, sink) = controller_for(inputs, &sink, "Noted; answering in your tuned voice.");
        let (outcome, acts) = drain_collecting(&c);
        drop(c);
        let _ = std::fs::remove_dir_all(&dir_main);
        if !bound_here(&home) {
            obs.push(format!(
                "main leg: env claim lost mid-turn at attempt {attempt}"
            ));
            return LegResult::Crossing;
        }
        if !matches!(outcome, TurnOutcome::Applied { .. }) {
            return LegResult::Fatal(format!("main-chat leg must Apply: {outcome:?}"));
        }
        let main_instr = match sink.lock().unwrap().take() {
            Some(i) => i,
            None => {
                return LegResult::Fatal("main-chat turn captured no instructions".into());
            }
        };
        let intro = main_instr.find(PERSONA_MARKER);
        let layout_ok = main_instr.matches(PERSONA_MARKER).count() == 1
            && intro
                .zip(main_instr.find("the closing JSON fence."))
                .is_some_and(|(intro, fence)| fence < intro)
            && main_instr
                .find(PLANNER_POLICY)
                .is_some_and(|pol| pol + PLANNER_POLICY.len() <= intro.expect("marker found"));
        if !(main_instr.ends_with(SENTINEL_A) && layout_ok) {
            obs.push(format!(
                "main leg unbound at attempt {attempt}: ends_with_sentinel={}, acts={:?}",
                main_instr.ends_with(SENTINEL_A),
                acts
            ));
            return LegResult::Crossing;
        }
        // Negative of the diagnostic channel is a flat test
        // property (FATAL, not an env clue): a healthy load
        // emits no `Persona note:` activity. Disqualifying only
        // note-shaped activities keeps future benign mid-turn
        // notices from masquerading as a home crossing and
        // burning the redo budget.
        if !acts.iter().all(|a| !a.starts_with("Persona note:")) {
            return LegResult::Fatal(format!(
                "healthy load unexpectedly surfaced a diagnostic note: {acts:?}"
            ));
        }
        if !main_instr.contains("PRODUCT INTENT INTERVIEW")
            || main_instr.contains("TASK CONVERSATION MODE:")
        {
            return LegResult::Fatal(
                "main-mode topology drifted: interview section missing or task banner leaked"
                    .into(),
            );
        }
        LegResult::Bound
    });

    // Scoped task-conversation leg: the SAME stored sentinel
    // must ride the scoped mode too, with the banner indexing
    // strictly before the layer.
    run_leg(|attempt, obs| {
        let (mut inputs, dir_task) = inputs_for("pj_task", "Which SSO route do you recommend?");
        let home = LockedPersonaHome::new(&format!("modes_task_{attempt}"));
        save_persona(SENTINEL_A).expect("sentinel save must succeed");
        if !bound_here(&home) {
            obs.push(format!(
                "task leg: pre-flight env check lost at attempt {attempt}"
            ));
            let _ = std::fs::remove_dir_all(&dir_task);
            return LegResult::Crossing;
        }
        inputs.state.items.push(crate::domain::OpenItem::new(
            "CLR-001".into(),
            crate::domain::Priority::Normal,
            crate::domain::ItemKind::Question,
            "Engineering".into(),
            None,
            "Pick the SSO route.".into(),
            "Rollout blocks on it.".into(),
        ));
        let key = inputs
            .state
            .items
            .last()
            .expect("CLR-001 pushed")
            .conversation_key()
            .to_string();
        let sink: Arc<std::sync::Mutex<Option<String>>> = Arc::new(std::sync::Mutex::new(None));
        let scoped = TurnController::start_scoped(
            inputs,
            Box::new(CaptureHarness {
                raw: canned_raw("Recommendation: the least-friction route."),
                sink: Arc::clone(&sink),
            }),
            Some(key),
        );
        let (outcome, _acts) = drain_collecting(&scoped);
        drop(scoped);
        let _ = std::fs::remove_dir_all(&dir_task);
        if !bound_here(&home) {
            obs.push(format!(
                "task leg: env claim lost mid-turn at attempt {attempt}"
            ));
            return LegResult::Crossing;
        }
        if !matches!(outcome, TurnOutcome::Applied { .. }) {
            return LegResult::Fatal(format!("scoped task leg must Apply: {outcome:?}"));
        }
        let task_instr = match sink.lock().unwrap().take() {
            Some(i) => i,
            None => return LegResult::Fatal("scoped turn captured no instructions".into()),
        };
        let intro = task_instr.find(PERSONA_MARKER);
        let layout_ok = task_instr.matches(PERSONA_MARKER).count() == 1
            && intro
                .zip(task_instr.find("TASK CONVERSATION MODE:"))
                .is_some_and(|(intro, banner)| banner < intro);
        if !(task_instr.ends_with(SENTINEL_A) && layout_ok) {
            obs.push(format!(
                "task leg unbound at attempt {attempt}: ends_with_sentinel={}, marker_count={}",
                task_instr.ends_with(SENTINEL_A),
                task_instr.matches(PERSONA_MARKER).count()
            ));
            return LegResult::Crossing;
        }
        if task_instr.contains("PRODUCT INTENT INTERVIEW") {
            return LegResult::Fatal("task mode must omit the main-mode interview section".into());
        }
        LegResult::Bound
    });
}

#[test]
fn unservable_persona_falls_back_to_shipped_default_with_surfaced_diagnostic() {
    let _serial = LEG_SERIALIZER
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());

    // Leg 1: persona.md overwritten with NON-UTF-8 bytes (a 0xFF
    // then 0xFE header plus junk).
    run_leg(|attempt, obs| {
        let (inputs, dir) = inputs_for("pj_corrupt", "How does the SSO draft stand?");
        let home = LockedPersonaHome::new(&format!("fb_corrupt_{attempt}"));
        let corrupt: &[u8] = &[0xFF, 0xFE, b'j', b'u', b'n', b'k'];
        std::fs::write(persona_path(), corrupt).expect("write the corrupt bytes");
        if !bound_here(&home) {
            obs.push(format!(
                "corrupt leg: pre-flight env check lost at attempt {attempt}"
            ));
            let _ = std::fs::remove_dir_all(&dir);
            return LegResult::Crossing;
        }
        let sink: Arc<std::sync::Mutex<Option<String>>> = Arc::new(std::sync::Mutex::new(None));
        let (c, sink) = controller_for(inputs, &sink, "Standing reported; nothing to decide yet.");
        let (outcome, acts) = drain_collecting(&c);
        drop(c);
        let _ = std::fs::remove_dir_all(&dir);
        if !bound_here(&home) {
            obs.push(format!(
                "corrupt leg: env claim lost mid-turn at attempt {attempt}"
            ));
            return LegResult::Crossing;
        }
        if !matches!(outcome, TurnOutcome::Applied { .. }) {
            return LegResult::Fatal(format!(
                "corrupt-file leg must still APPLY (never a harness failure): {outcome:?}"
            ));
        }
        let instr = match sink.lock().unwrap().take() {
            Some(i) => i,
            None => return LegResult::Fatal("corrupt leg captured no instructions".into()),
        };
        if instr.matches(PERSONA_MARKER).count() != 1 {
            return LegResult::Fatal(
                "the defaulted layer must still be labeled exactly once".into(),
            );
        }
        let path = persona_path().to_string_lossy().to_string();
        // The store diagnostic travels VERBATIM through the
        // notification: full message frame anchored on this
        // home's path.
        let verbatim_noticed = acts.iter().any(|a| {
            a.starts_with("Persona note: persona file ")
                && a.contains(&path)
                && a.ends_with(" is not valid UTF-8; serving the shipped default")
        });
        if !(instr.ends_with(SHIPPED_DEFAULT_PERSONA) && verbatim_noticed) {
            obs.push(format!(
                "corrupt leg unbound at attempt {attempt}: ends_with_default={}, acts={:?}",
                instr.ends_with(SHIPPED_DEFAULT_PERSONA),
                acts
            ));
            return LegResult::Crossing;
        }
        LegResult::Bound
    });

    // Leg 2: persona.md deleted outright (presence established
    // first so the removal is the operative state).
    run_leg(|attempt, obs| {
        let (inputs, dir) = inputs_for("pj_deleted", "Same question, please.");
        let home = LockedPersonaHome::new(&format!("fb_deleted_{attempt}"));
        save_persona("# doomed draft\n").expect("setup save must succeed");
        std::fs::remove_file(persona_path()).expect("delete-leg setup");
        assert!(!persona_path().exists(), "delete-leg precondition");
        if !bound_here(&home) {
            obs.push(format!(
                "delete leg: pre-flight env check lost at attempt {attempt}"
            ));
            let _ = std::fs::remove_dir_all(&dir);
            return LegResult::Crossing;
        }
        let sink: Arc<std::sync::Mutex<Option<String>>> = Arc::new(std::sync::Mutex::new(None));
        let (c, sink) = controller_for(inputs, &sink, "Same standing; still nothing to decide.");
        let (outcome, acts) = drain_collecting(&c);
        drop(c);
        let _ = std::fs::remove_dir_all(&dir);
        if !bound_here(&home) {
            obs.push(format!(
                "delete leg: env claim lost mid-turn at attempt {attempt}"
            ));
            return LegResult::Crossing;
        }
        if !matches!(outcome, TurnOutcome::Applied { .. }) {
            return LegResult::Fatal(format!("delete leg must still APPLY: {outcome:?}"));
        }
        let instr = match sink.lock().unwrap().take() {
            Some(i) => i,
            None => return LegResult::Fatal("delete leg captured no instructions".into()),
        };
        let path = persona_path().to_string_lossy().to_string();
        // Same verbatim bar for the seed-path diagnostic.
        let noticed = acts.iter().any(|a| {
            a.starts_with("Persona note: persona file ")
                && a.contains(&path)
                && a.ends_with(" was absent; seeded the shipped default")
        });
        if !(instr.ends_with(SHIPPED_DEFAULT_PERSONA) && noticed) {
            obs.push(format!(
                "delete leg unbound at attempt {attempt}: ends_with_default={}, acts={:?}",
                instr.ends_with(SHIPPED_DEFAULT_PERSONA),
                acts
            ));
            return LegResult::Crossing;
        }
        LegResult::Bound
    });
}

#[test]
fn resaved_sentinel_reaches_the_second_controller_without_cache_or_restart() {
    let _serial = LEG_SERIALIZER
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());

    run_leg(|attempt, obs| {
        // Both turns live under ONE claimed home: turn one stores
        // sentinel A, then B is stored IN-PROCESS — no relaunch,
        // no restart — and turn two under a FRESH controller must
        // carry it.
        let home = LockedPersonaHome::new(&format!("nocache_{attempt}"));
        save_persona(SENTINEL_A).expect("sentinel A save must succeed");
        if !bound_here(&home) {
            obs.push(format!(
                "nocache leg: pre-flight env check lost at attempt {attempt}"
            ));
            return LegResult::Crossing;
        }
        let (inputs, dir_a) = inputs_for("pj_nocache_a", "First pass, tuned voice.");
        let sink_a: Arc<std::sync::Mutex<Option<String>>> = Arc::new(std::sync::Mutex::new(None));
        let (c, sink_a) = controller_for(inputs, &sink_a, "First pass acknowledged.");
        let (outcome, _acts) = drain_collecting(&c);
        drop(c);
        if !bound_here(&home) {
            obs.push(format!(
                "nocache leg: env claim lost mid-turn one at attempt {attempt}"
            ));
            let _ = std::fs::remove_dir_all(&dir_a);
            return LegResult::Crossing;
        }
        if !matches!(outcome, TurnOutcome::Applied { .. }) {
            return LegResult::Fatal(format!("turn one must Apply: {outcome:?}"));
        }
        let first = match sink_a.lock().unwrap().take() {
            Some(i) => i,
            None => return LegResult::Fatal("turn one captured no instructions".into()),
        };
        let _ = std::fs::remove_dir_all(&dir_a);

        save_persona(SENTINEL_B).expect("sentinel B save must succeed");
        if !bound_here(&home) {
            obs.push(format!(
                "nocache leg: env check lost between re-save and turn two at attempt {attempt}"
            ));
            return LegResult::Crossing;
        }

        let (inputs, dir_b) = inputs_for("pj_nocache_b", "Second pass, retuned voice.");
        let sink_b: Arc<std::sync::Mutex<Option<String>>> = Arc::new(std::sync::Mutex::new(None));
        let (c, sink_b) = controller_for(inputs, &sink_b, "Second pass acknowledged.");
        let (outcome, _acts) = drain_collecting(&c);
        drop(c);
        let _ = std::fs::remove_dir_all(&dir_b);
        if !bound_here(&home) {
            obs.push(format!(
                "nocache leg: env claim lost mid-turn two at attempt {attempt}"
            ));
            return LegResult::Crossing;
        }
        if !matches!(outcome, TurnOutcome::Applied { .. }) {
            return LegResult::Fatal(format!("turn two must Apply: {outcome:?}"));
        }
        let second = match sink_b.lock().unwrap().take() {
            Some(i) => i,
            None => return LegResult::Fatal("turn two captured no instructions".into()),
        };
        if !(first.ends_with(SENTINEL_A) && second.ends_with(SENTINEL_B) && first != second) {
            obs.push(format!(
                "nocache leg unbound at attempt {attempt}: first_ends_A={}, second_ends_B={}",
                first.ends_with(SENTINEL_A),
                second.ends_with(SENTINEL_B)
            ));
            return LegResult::Crossing;
        }
        // Divergence topology: the captures differ ONLY from the
        // persona-layer slot onward.
        let pa = match first.find(PERSONA_MARKER) {
            Some(p) => p,
            None => return LegResult::Fatal("turn one intro missing".into()),
        };
        let pb = match second.find(PERSONA_MARKER) {
            Some(p) => p,
            None => return LegResult::Fatal("turn two intro missing".into()),
        };
        if first[..pa] != second[..pb] {
            return LegResult::Fatal("standing prefixes must compare byte-equal".into());
        }
        if first[pa..pa + PERSONA_LAYER_INTRO.len()] != second[pb..pb + PERSONA_LAYER_INTRO.len()] {
            return LegResult::Fatal(
                "the layer preamble must be byte-identical across turns".into(),
            );
        }
        if &first[pa + PERSONA_LAYER_INTRO.len()..] != SENTINEL_A {
            obs.push(
                "nocache: turn-one document slice is neither sentinel nor default (crossing)"
                    .into(),
            );
            return LegResult::Crossing;
        }
        if &second[pb + PERSONA_LAYER_INTRO.len()..] != SENTINEL_B {
            obs.push("nocache: turn-two document slice is not sentinel B (crossing)".into());
            return LegResult::Crossing;
        }
        LegResult::Bound
    });
}
