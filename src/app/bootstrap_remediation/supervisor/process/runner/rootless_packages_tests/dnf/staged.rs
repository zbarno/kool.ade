#[path = "staged/failures.rs"]
mod failures;
#[path = "staged/harness.rs"]
mod harness;
#[path = "staged/success.rs"]
mod success;

// This fake-runner suite covers the rootless DNF transaction state machine,
// including exact key inventory, path/hash revalidation, isolated cache, and
// offline installation. It cannot prove DNF5/RPM or privileged process behavior.
// Live validation needs a disposable Fedora 45 VM, DNF5/RPM, network access to
// Fedora and Docker repositories, and elevated execution; setup includes
// downloading and installing the full dependency closure.
