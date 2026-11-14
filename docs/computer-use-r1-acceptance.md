# Computer Use R1 offline acceptance infrastructure

This page records the offline disposable-VM automated gate. It is a separate
evidence slice under [#392](https://github.com/zbarno/kool.ade/issues/392); it
does not close the cross-tier issue or certify later release tiers.

## Trusted runner contract

The **Computer Use R1 real KVM acceptance** is a manually dispatched workflow
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
The preflight checks the runner's current cgroup and its ancestors, selecting
the nearest writable delegated subtree with all three controllers enabled;
this supports runners launched in a child scope under a delegated systemd
service. Missing prerequisites fail the required run. The preflight does not
change permissions or host configuration. After preflight, the workflow runs the
`computer_use_r1_real_kvm_acceptance` Rust test, which downloads and verifies
the pinned signed x86_64 guest image, creates a disposable overlay, boots the
guest through the production QEMU/KVM backend, authenticates the companion,
launches Chromium through controller-scoped authority, captures authenticated
guest and QEMU display frames, and releases the VM.

When the input is false, the workflow explicitly reports that real-host
preflight was not requested. Such a run is not KVM or guest acceptance
evidence. The required path fails until a trusted, isolated runner is
registered.

On 2026-11-14, the same opt-in acceptance test passed locally on a real
Pop!_OS 24.04 LTS x86_64 KVM host running kernel
`7.2.15-76070215-generic` and QEMU/qemu-img 8.2.2. It verified published
generation 19 (`sha256:53ca225f40a0229085cf8da26b8e205b94052679fc39e0903a6999e6dee706ec`),
authenticated companion protocol 1, launched Chromium, received 1280×800 guest
and QEMU display frames, and released the VM. The exact command/result and
host details are recorded in [#392](https://github.com/zbarno/kool.ade/issues/392#issuecomment-6872727893).
The test ran locally because no online Actions runner carries the
`computer-use-kvm` label; the manually dispatched workflow contract is
unchanged.

## Evidence boundary

The host preflight records OS/architecture, tool versions, KVM device
accessibility, and delegated controller availability. In addition to the
boot/launch/screenshot/display/release run above, the opt-in acceptance test
passed on 2026-11-14 at source commit `dbdf4828bf4a63801189e483bc1b04e644b86c39`.
It sent a guest mouse click, refreshed authenticated frame metadata between
actions, used Ctrl+L and typed/navigated a local data URL, then exercised
human-controlled guest input and agent handback while checking that the prior
actor capabilities were revoked. The cached-image run took 46.51 seconds and
reported 1280×800 guest and QEMU display frames. This did not exercise the live
Task Details viewer UI or a manually operated external handoff.

File transfer, guest/host isolation, QA repair, crash recovery, performance
measurements, and accessibility remain unverified. The test's opt-in status
means ordinary CI can skip it; only the locally observed KVM run is evidence
here. #392 remains open while its other acceptance criteria are completed.

The existing ordinary CI suite continues to cover deterministic component
tests and the rootless Docker boundary. Its mocks and synthetic browser
fixtures do not establish QEMU/KVM behavior. R1 release eligibility remains
closed until the remaining integration, isolation, takeover, recovery, and
performance evidence is recorded in #392.
