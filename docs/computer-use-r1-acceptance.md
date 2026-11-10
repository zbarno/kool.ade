# Computer Use R1 offline acceptance infrastructure

This page records the offline disposable-VM automated gate. It is a separate
evidence slice under [#392](https://github.com/zbarno/kool.ade/issues/392); it
does not close the cross-tier issue or certify later release tiers.

## Trusted runner contract

The **Computer Use R1 KVM host preflight** is a manually dispatched workflow
with a required `require_kvm_host` choice. It only accepts `main` as the
workflow ref, so unreviewed pull request code never runs on the self-hosted
host. A required run first checks that at least one online runner is
registered with all of these labels:

`self-hosted`, `linux`, `x64`, `computer-use-kvm`

It then runs the read-only
[`check-computer-use-r1-kvm-host.py`](../scripts/check-computer-use-r1-kvm-host.py)
preflight on that runner. The runner must be an unprivileged Linux x86_64 host
with read/write `/dev/kvm`, trusted root-owned `/usr/bin/qemu-system-x86_64`
and `/usr/bin/qemu-img`, QEMU's reported KVM accelerator, no detected nested
hypervisor, and delegated cgroup v2 `cpu`, `memory`, and `pids` controllers.
Missing prerequisites fail the required run. The preflight does not change
permissions or host configuration.

When the input is false, the workflow explicitly reports that real-host
preflight was not requested. Such a run is not KVM or guest acceptance
evidence. The required path fails until a trusted, isolated runner is
registered.

## Evidence boundary

The host preflight records OS/architecture, tool versions, KVM device
accessibility, and delegated controller availability. It does **not** boot a
trusted guest, validate its signed image, perform a companion handshake or GUI
action, exercise display streaming or takeover, prove guest/host isolation, or
test cleanup and crash recovery. Those end-to-end scenarios remain unverified
until an R1 guest acceptance test is connected to the trusted runner. This
workflow must not be cited as passing real-VM acceptance.

The existing ordinary CI suite continues to cover deterministic component
tests and the rootless Docker boundary. Its mocks and synthetic browser
fixtures do not establish QEMU/KVM behavior. R1 release eligibility remains
closed until end-to-end guest evidence is attached to #392.
