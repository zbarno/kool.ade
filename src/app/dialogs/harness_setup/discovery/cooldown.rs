//! Serializes discovery scans across every `DlgHarnessSetup` instance.
//!
//! Each dialog construction historically forced a fresh sweep, and re-entrant
//! refreshes cancelled the scan in flight. With nothing throttling starts, a
//! feedback loop (a parked task repeatedly requesting capability evidence)
//! can restart sweeps faster than they complete, leaving the dialog stuck on
//! "checking…" forever. A global, coarse cooldown bounds scan starts so the
//! previous sweep always has a chance to finish, and declines are surfaced as
//! feedback instead of invisible no-ops.

use std::sync::atomic::{AtomicU64, Ordering};

/// Minimum elapsed time between two scan starts, in Unix milliseconds.
///
/// Chosen larger than the shortest realistic provider sweep so a refused
/// refresh can never starve a running one; the UI surfaces the refusal with
/// "again shortly" wording rather than pretending the click was lost.
pub(super) const REFRESH_COOLDOWN_MS: u64 = 2000;

static LAST_SCAN_START_MS: AtomicU64 = AtomicU64::new(0);

thread_local! {
    /// Test override for the cooldown window; `None` keeps the production value.
    static WINDOW_MS_OVERRIDE: std::cell::Cell<Option<u64>> = const { std::cell::Cell::new(None) };
    /// Test override for the wall clock feeding the window math; `0` means unset.
    static TESTING_NOW_MS: std::cell::Cell<u64> = const { std::cell::Cell::new(0) };
}

pub(super) enum ScanWindow {
    Accepted,
    /// The cooldown has not elapsed; `remaining_seconds` is at least one.
    Declined {
        remaining_seconds: String,
    },
}

fn window_ms() -> u64 {
    WINDOW_MS_OVERRIDE.with(|slot| slot.get().unwrap_or(REFRESH_COOLDOWN_MS))
}

/// Reserve the next scan start at `now_ms`, or decline while the previous
/// start is still inside the cooldown window.
///
/// Uses compare-and-swap so concurrent starters contend for a single window
/// slot instead of stacking launches on top of each other.
pub(super) fn acquire_scan_window(now_ms: u64) -> ScanWindow {
    let window = window_ms();
    let mut claimed = LAST_SCAN_START_MS.load(Ordering::Relaxed);
    loop {
        let elapsed = now_ms.saturating_sub(claimed);
        if window > 0 && elapsed < window {
            let remaining_seconds = (window - elapsed).div_ceil(1000);
            return ScanWindow::Declined {
                remaining_seconds: remaining_seconds.to_string(),
            };
        }
        match LAST_SCAN_START_MS.compare_exchange_weak(
            claimed,
            now_ms,
            Ordering::Relaxed,
            Ordering::Relaxed,
        ) {
            Ok(_) => return ScanWindow::Accepted,
            Err(current) => claimed = current,
        }
    }
}

/// Wall clock feeding the cooldown decisions. The production value is the
/// shared Unix-millisecond reader; tests may pin a synthetic instant so
/// "later frame" scenarios need not sleep.
pub(in crate::app::dialogs::harness_setup) fn effective_now_ms() -> u64 {
    let pinned = TESTING_NOW_MS.with(|slot| slot.get());
    if pinned > 0 {
        pinned
    } else {
        crate::persistence::harness_settings::current_unix_ms()
    }
}

/// Test seam: pin the wall clock fed into the cooldown decisions for the
/// duration of `run` (nesting restores outward, like `with_window`).
#[cfg(test)]
pub(in crate::app::dialogs::harness_setup) fn with_testing_now<R>(
    now_ms: u64,
    run: impl FnOnce() -> R,
) -> R {
    let previous = TESTING_NOW_MS.with(|slot| slot.get());
    TESTING_NOW_MS.with(|slot| slot.set(now_ms));
    struct Restore(u64);
    impl Drop for Restore {
        fn drop(&mut self) {
            TESTING_NOW_MS.with(|slot| slot.set(self.0));
        }
    }
    let _guard = Restore(previous);
    run()
}

/// Test seam: seed the last-scan timestamp without racing a real scan.
#[cfg(test)]
pub(in crate::app::dialogs::harness_setup) fn testing_seed_last_scan_start(ms: u64) {
    LAST_SCAN_START_MS.store(ms, Ordering::Relaxed);
}

/// Test seam: temporarily resize the cooldown window (restores on drop).
#[cfg(test)]
pub(in crate::app::dialogs::harness_setup) fn with_window<R>(
    ms: u64,
    run: impl FnOnce() -> R,
) -> R {
    let previous = WINDOW_MS_OVERRIDE.with(|slot| slot.get());
    WINDOW_MS_OVERRIDE.with(|slot| slot.set(Some(ms)));
    struct Restore(Option<u64>);
    impl Drop for Restore {
        fn drop(&mut self) {
            WINDOW_MS_OVERRIDE.with(|slot| slot.set(self.0));
        }
    }
    let _guard = Restore(previous);
    run()
}

#[cfg(test)]
mod tests {
    use super::*;

    trait AssertAccepted {
        fn is_accepted(&self) -> bool;
    }

    impl AssertAccepted for ScanWindow {
        fn is_accepted(&self) -> bool {
            matches!(self, ScanWindow::Accepted)
        }
    }

    #[test]
    fn a_second_start_inside_the_window_is_declined_with_remaining_time() {
        with_window(REFRESH_COOLDOWN_MS, || {
            testing_seed_last_scan_start(100_000);
            let ScanWindow::Declined { remaining_seconds } = acquire_scan_window(100_500) else {
                panic!("second start 500ms later must be declined");
            };
            // 2000ms window, 500ms elapsed -> 1500ms left -> rounds up to 2s.
            assert_eq!(remaining_seconds, "2");
        });
    }

    #[test]
    fn starts_are_accepted_again_once_the_window_has_elapsed() {
        with_window(REFRESH_COOLDOWN_MS, || {
            testing_seed_last_scan_start(200_000);
            // Exactly at the boundary the window has elapsed: accepted.
            assert!(matches!(
                acquire_scan_window(200_000 + REFRESH_COOLDOWN_MS),
                ScanWindow::Accepted
            ));
        });
    }

    #[test]
    fn a_clock_step_back_never_overflows_or_starves_forever() {
        with_window(REFRESH_COOLDOWN_MS, || {
            testing_seed_last_scan_start(400_000);
            let ScanWindow::Declined { remaining_seconds } = acquire_scan_window(1) else {
                panic!("rewound clock inside window must be declined");
            };
            // Saturation caps the remainder at the window itself.
            assert!(remaining_seconds.parse::<u64>().unwrap() <= 2);
        });
    }

    #[test]
    fn a_zero_window_admits_consecutive_starts_for_test_doubles() {
        with_window(0, || {
            testing_seed_last_scan_start(500_000);
            assert!(acquire_scan_window(500_000).is_accepted());
            assert!(acquire_scan_window(500_000).is_accepted());
        });
    }

    #[test]
    fn contending_starters_win_single_slots_per_window() {
        with_window(500, || {
            testing_seed_last_scan_start(0);
            let now: u64 = 1_000_000;
            let winners = std::sync::Arc::new(std::sync::atomic::AtomicUsize::new(0));
            let mut handles = Vec::new();
            for _ in 0..8 {
                let winners = winners.clone();
                handles.push(std::thread::spawn(move || {
                    if acquire_scan_window(now).is_accepted() {
                        winners.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
                    }
                }));
            }
            for handle in handles {
                handle.join().unwrap();
            }
            // Everyone races the same instant: the CAS admits no duplicates
            // for a single window slot.
            assert_eq!(winners.load(std::sync::atomic::Ordering::SeqCst), 1);
        });
    }
}
