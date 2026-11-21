//! Dedicated NON-TOKIO standard thread hosting contained provider runs
//! (#591 phase 4, wiring slice A).
//!
//! Why a lane at all: the production driver bundles its own
//! CURRENT-THREAD tokio runtime (`ProviderContainerDriver::connect`
//! builds one). Nested under the app's multithread tokio reactor that
//! runtime would panic outright, and even where it would not it would
//! interfere with reactor work. The lane's standard thread hosts no
//! reactor: every job constructs its own runtime stack synchronously and
//! blocks inside it — legal, bounded (the driver enforces absolute wall
//! caps on every engine segment), and invisible to the app's async world.
//!
//! Throughput contract (v1, documented): ONE contained run at a time
//! process-wide; further submissions queue on the mailbox. Worker-role
//! implementation turns are sequential in this build, so the
//! serialization costs nothing real.
//!
//! Failure semantics: transport errors (worker death, respawn races)
//! surface as a typed error — never swallowed. Job bodies are fail-closed
//! by the driver itself (hard caps, verified-absence teardown), so even a
//! worker thread that dies mid-job cannot strand a container beyond the
//! driver's own deadlines; the lane reports such a loss as a transport
//! fault to the caller.
use std::{
    sync::{Mutex, OnceLock, PoisonError, mpsc},
    thread,
};

use crate::{
    execution::docker::provider_session::CapturedExecReport,
    harness::execution_security::ContainedLaunchError,
};

/// Everything a contained run can fail with while IN FLIGHT. Kept apart
/// from the session error type so callers can distinguish "we could not
/// even talk to the lane" from "the engine said no".
// Pure data: derived Clone lets the lane relay outcomes without copying
// live resources; Debug makes faults diagnosable end-to-end.
#[derive(Debug, Clone)]
pub(crate) enum ContainedRunFault {
    /// The executor factory failed before any engine traffic.
    Connect(String),
    /// The contained launch sequencer failed; carries its full structured
    /// detail, including verified-teardown status.
    Launch(ContainedLaunchError),
}

impl ContainedRunFault {
    pub(crate) fn describe(&self) -> String {
        match self {
            Self::Connect(cause) => format!("contained-exec connect: {cause}"),
            Self::Launch(err) => match err {
                ContainedLaunchError::UnsupportedProvider { provider } => {
                    format!("containment refuses provider {provider}")
                }
                ContainedLaunchError::InsufficientCapability { obstructions } => {
                    format!("containment admission insufficient: {obstructions:?}")
                }
                ContainedLaunchError::SpecRejected { detail } => {
                    format!("containment spec rejected: {detail}")
                }
                ContainedLaunchError::Engine {
                    detail,
                    teardown_ok,
                } => format!(
                    "containment engine failure: {detail} \
                     (verified teardown: {})",
                    if *teardown_ok { "yes" } else { "NO" }
                ),
            },
        }
    }
}

type LaneJob = Box<dyn FnOnce() -> Result<CapturedExecReport, ContainedRunFault> + Send + 'static>;

/// Mailbox sender paired with its worker's join handle (consumed only
/// by the death-quarantine test's full-teardown proof; production never
/// joins, and dropping the handle detaches harmlessly).
type LaneSlot = Option<(mpsc::Sender<LaneJob>, Option<thread::JoinHandle<()>>)>;

static LANE: OnceLock<Mutex<LaneSlot>> = OnceLock::new();

fn slot() -> &'static Mutex<LaneSlot> {
    LANE.get_or_init(|| Mutex::new(None))
}

fn with_slot<F, R>(f: F) -> R
where
    F: FnOnce(&mut LaneSlot) -> R,
{
    let mut guard = slot().lock().unwrap_or_else(PoisonError::into_inner);
    f(&mut guard)
}

/// The waiting-side handle: the caller's result channel, through which
/// the lane relays the run's typed outcome.
type LaneResultRx = mpsc::Receiver<Result<CapturedExecReport, ContainedRunFault>>;

fn ensure_worker(slot_guard: &mut LaneSlot) -> Result<&mpsc::Sender<LaneJob>, ContainedRunFault> {
    if slot_guard.is_none() {
        let (tx, rx) = mpsc::channel::<LaneJob>();
        let spawned = thread::Builder::new()
            .name("koolade-contained-exec".into())
            .spawn(move || worker_loop(rx));
        let handle = if let Err(spawn_err) = &spawned {
            // A resource-exhausted spawn must NOT take down the (possibly
            // UI-facing) calling thread: typed transport fault, and the
            // slot STAYS EMPTY so the next submission retries the spawn.
            return Err(ContainedRunFault::Connect(format!(
                "contained-exec lane could not spawn its worker thread ({spawn_err}); \
                 the slot stays empty and the next submission retries"
            )));
        } else {
            spawned.expect("classified Err above")
        };
        *slot_guard = Some((tx, Some(handle)));
    }
    Ok(slot_guard
        .as_ref()
        .map(|(tx, _)| tx)
        .expect("either pre-set or spawned moments ago on the Ok path"))
}

fn worker_loop(rx: mpsc::Receiver<LaneJob>) {
    while let Ok(job) = rx.recv() {
        // The submitted closure relays its outcome clone over the caller's
        // result channel BEFORE returning it; dropping the worker-side
        // value here is the intentional half of that relay.
        let _ = job();
    }
}

/// Enqueue one contained run and wait for its (always terminating)
/// result. A dead worker's broken mailbox is detected on the send, the
/// worker slot is RESET (relaunched by the next submission), and THIS
/// caller receives a typed transport fault — never a silent drop.
pub(crate) fn submit(job: LaneJob) -> Result<CapturedExecReport, ContainedRunFault> {
    let (result_rx, transmitted) = with_slot(
        |slot_guard| -> Result<(LaneResultRx, bool), ContainedRunFault> {
            let tx = ensure_worker(slot_guard)?;
            let (result_tx, result_rx) = mpsc::channel();
            let wrapped = move || {
                let outcome = job();
                // Relay idiom: the receiver gets a clone over the channel;
                // the worker-side return value is intentionally dropped.
                let _ = result_tx.send(outcome.clone());
                outcome
            };
            let transmitted = tx.send(Box::new(wrapped)).is_ok();
            if !transmitted {
                // Broken mailbox: sever the dead sender so the next
                // submission launches a replacement worker.
                *slot_guard = None;
            }
            Ok((result_rx, transmitted))
        },
    )?;
    if !transmitted {
        return Err(ContainedRunFault::Connect(
            "contained-exec lane could not enqueue the run; the worker was reset and \
             the next submission relaunches it"
                .into(),
        ));
    }
    result_rx.recv().unwrap_or_else(|_dropped| {
        Err(ContainedRunFault::Connect(
            "contained-exec lane worker vanished mid-run; driver deadlines still bound \
             any orphaned engine work"
                .into(),
        ))
    })
}

#[cfg(test)]
pub(crate) mod test_support {
    use std::sync::{Mutex, MutexGuard, OnceLock, PoisonError};

    /// The lane is PROCESS-GLOBAL, and the worker-death test transiently
    /// sacrifices it; every lane CONSUMER (direct lane tests AND the
    /// wrapper tests that drive `execute`) must hold this guard across
    /// its submissions so no bystander observes the sacrificial window.
    pub(crate) fn lane_consumer_exclusive() -> MutexGuard<'static, ()> {
        static LOCK: OnceLock<Mutex<()>> = OnceLock::new();
        LOCK.get_or_init(|| Mutex::new(()))
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
    }

    /// Retire the current slot ATOMICALLY (mailbox sender + worker handle)
    /// and JOIN the (by premise already dead) worker, proving its frames —
    /// including the mailbox receiver — are fully popped. Called ONLY by
    /// the death-quarantine test between its phases; the next submission
    /// spawns a fresh worker. Joining a still-alive worker would block, so
    /// the caller must observe phase-1 transport loss first.
    /// JOIN the current worker (whose death the caller has already
    /// witnessed as phase-1 transport loss) WITHOUT draining the slot:
    /// the dead sender stays in place so the caller can still observe
    /// the broken-mailbox reset on its next send. Joining a live worker
    /// would block, hence the witness requirement in the doc.
    pub(crate) fn retire_current_worker() {
        super::with_slot(|slot| {
            if let Some(join) = slot.as_mut().and_then(|(_, handle)| handle.take()) {
                let _ = join.join();
            }
        });
    }
}

#[cfg(test)]
#[path = "contained_lane/tests.rs"]
mod tests;
