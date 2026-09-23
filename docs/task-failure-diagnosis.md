# Diagnosing interrupted task implementation

Task cards show the failure cause and offer expandable, copyable failure details. A task that has a saved running status but no current worker is labeled interrupted. Resolve the reported cause and use Resume implementation; the existing worktree is reused.

Before implementation and verification, Packet checks for at least 1 GiB of available space on the relevant filesystem. This is a minimum start guard, not a reservation: a large build can require considerably more. Check both the repository/worktree volume and any separate build-output volume. Remove only rebuildable caches when reclaiming space; preserve worktrees and implementation evidence.

If a worker exits without returning a result, Packet reports a failure and releases its queue slot. If saving error evidence or task state also fails, the in-memory failure includes both the original cause and the persistence failure. Copy that failure before closing Packet when storage is unavailable.

## September 22 investigation

The development volume had zero available bytes. The most recent CHG-003 dual-instance verification task retained an Implementing state, an empty harness-error file, and empty state/activity temporary files. Its recorded activity reported linker bus errors and missing command output before it stopped. Disk exhaustion is directly established; the individual linker errors were not independently reproduced.

`cargo clean` removed rebuildable output from the main checkout and restored approximately 8.6 GiB. Existing implementation worktrees, reports, and historical evidence were preserved. Validation builds for this repair use `/tmp/packet-repair-target` on the separate root filesystem.

## September 23 automatic cleanup repair

Completed-task cleanup is now implemented and runs after completion and in periodic background maintenance. It verifies remote publication and worktree identity, refuses changed or locked worktrees, and preserves all implementation reports and verified commits. Failed cleanup remains visible on the completed card and retries while the project is open.

The first live maintenance passes reconciled all 23 completed records and removed 15 retained task/integration worktrees. The unfinished task and unrelated worktrees remained. Packet worktree usage fell from about 92 GiB to 5.6 GiB; development-volume free space rose from about 7.8 GiB to 95 GiB. All 327 checked report, response, correction, and verification files retained identical hashes.

## GUI-test cleanup terminating the supervisor

The September 23 GUI witness helper `/tmp/swrt7/driver_lib.py` used
`pgrep -f target/debug/packet` and sent SIGTERM to every match. This included the
operator's supervising Packet app, not just test windows. The latest harness
stream (`1790188843241390888-events.jsonl`) stops during the `/tmp/swrt8` smoke
run; that transcript reaches global cleanup after its successful window check
at 15:16:59. This explains the terminal's `Terminated` message. Rust panic hooks
do not handle this signal.

The live helper and its leg runner now use `scripts/owned_gui_processes.py`:
a unique inherited test-run marker, exact executable, private display, ancestor
exclusion, and pidfd signaling. Unrelated windows are excluded even if they use
the same executable and display. Historical transcripts and harness streams
were preserved. The interrupted task's current state records the diagnosis for
its next resume.

Every tool-enabled harness receives explicit process-ownership instructions,
including the supervising PID. These instructions are guidance, not an OS
sandbox: future generated scripts must still use ownership checks. Never use
machine-wide command-name matches to select processes for test cleanup.
