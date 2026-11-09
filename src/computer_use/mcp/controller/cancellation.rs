use std::sync::{
    Arc,
    atomic::{AtomicBool, Ordering},
};

use crate::computer_use::{ComputerMachineHandleV1, ComputerSessionV1};

use super::types::ComputerUseMcpControllerV1;

/// Interrupt a blocking guest operation when its owning request is cancelled.
/// Authority revocation waits for any action already inside `execute` to exit.
pub(super) fn run_cancellable<T>(
    controller: &ComputerUseMcpControllerV1,
    session: &ComputerSessionV1,
    handle: &ComputerMachineHandleV1,
    cancelled: &AtomicBool,
    operation: impl FnOnce() -> T,
) -> T {
    let Some(runtime) = controller.runtime.as_ref() else {
        return operation();
    };
    let backend = runtime.backend.clone();
    let registry = controller.registry.clone();
    let session = session.clone();
    let handle = handle.clone();
    run_with_cancellation(
        cancelled,
        || {
            backend
                .cancel(&session, &handle, super::session::unix_time_ms())
                .ok();
            registry.revoke(&session.identity.session_id).ok();
        },
        operation,
    )
}

fn run_with_cancellation<T>(
    cancelled: &AtomicBool,
    on_cancel: impl FnOnce() + Send,
    operation: impl FnOnce() -> T,
) -> T {
    std::thread::scope(|scope| {
        let stopped = Arc::new(AtomicBool::new(false));
        let stopped_ref = stopped.clone();
        let cancelled_ref = cancelled;
        let watcher = scope.spawn(move || {
            let mut on_cancel = Some(on_cancel);
            loop {
                if stopped_ref.load(Ordering::Acquire) {
                    return;
                }
                if cancelled_ref.load(Ordering::Acquire) {
                    if let Some(on_cancel) = on_cancel.take() {
                        on_cancel();
                    }
                    return;
                }
                std::thread::sleep(std::time::Duration::from_millis(10));
            }
        });
        let result = operation();
        stopped.store(true, Ordering::Release);
        let _ = watcher.join();
        result
    })
}

#[cfg(test)]
mod tests {
    use std::sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    };

    #[test]
    fn cancellation_interrupts_an_in_flight_operation() {
        let cancelled = AtomicBool::new(false);
        let interrupted = Arc::new(AtomicBool::new(false));
        let operation_interrupted = interrupted.clone();
        let result = super::run_with_cancellation(
            &cancelled,
            move || interrupted.store(true, Ordering::Release),
            || {
                cancelled.store(true, Ordering::Release);
                while !operation_interrupted.load(Ordering::Acquire) {
                    std::thread::yield_now();
                }
                "interrupted"
            },
        );
        assert_eq!(result, "interrupted");
    }
}
