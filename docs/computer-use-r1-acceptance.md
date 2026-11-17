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

On 2026-11-16, the same host passed the opt-in acceptance again against the
cached generation 19 image. This run verified the live session cgroup controls
after QEMU boot: `cpu.max=200000 100000`, `memory.max=2147483648`,
`memory.swap.max=0`, `memory.oom.group=1`, and `pids.max=128`, with live QEMU
tasks within that ceiling. Authenticated guest and QEMU display frames were
1280×800; mouse/keyboard input, takeover/handback, unsupported-accessibility
denial, artifact handoff, and cleanup also passed. The final run after
action-sequence correction took 55.44 seconds. The GUI acceptance waits up to
one second for the guest frame to reflect completed input actions. This is
one Linux host run, not resource-exhaustion stress evidence.

The 2026-11-16 real-VM QA rerun uses two invocations, each with a fresh
session and signed guest: the seeded defect was detected, then the corrected
fixture passed in its own VM. Guest staging allows only one approved app launch
per session, so reusing the first session would not model a new build retest.
Both runs authenticated the companion, captured guest/display frames, and
cleaned up. Exact screenshot digests and timings are in the
[release evidence matrix](computer-use-release-evidence.md). This is local
single-host evidence; no hosted KVM runner is registered.

Each run then killed the owned QEMU process, required collection to report a
stopped machine with no normal exit code, and called recovery on a fresh
backend instance. Recovery returned one matching event and removed the
session directory, cgroup, and overlay in 7 ms and 10 ms. This covers guest
process crash cleanup and backend reconstruction.

On 2026-11-16, the opt-in acceptance was extended to run the controller and
live VM in a separate test process. After authenticated interaction, takeover,
and the seeded QA driver, the controller wrote a readiness marker and was
terminated with SIGKILL. The test verified that the session-owned QEMU process
was still alive, then reconstructed the backend in the surviving process,
recovered exactly one matching event, and verified QEMU, the session directory,
cgroup, and overlay were removed. The full seeded-defect acceptance run
completed in 136.40 seconds and the corrected-build run in 132.75 seconds.
Controller restart recovery took 134 ms for each; the separate QEMU crash
recovery took 7 ms for each. This is full controller process restart evidence
on one local host, not hosted KVM execution or multi-host stress.

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

File transfer and broad guest/host isolation remain unverified. The corrected
QA build, QEMU crash cleanup, controller process restart, performance samples,
unsupported accessibility denial, and a bounded pids.max exhaustion check are
covered on this host. The task-limit check demonstrates one denied thread
creation and clean worker teardown; repeated, concurrent, memory-pressure, and
other-host exhaustion cases remain unverified. The test's opt-in status means
ordinary CI can skip it; only the locally observed KVM run is evidence here.
#392 remains open while its other acceptance criteria are completed.

The existing ordinary CI suite continues to cover deterministic component
tests and the rootless Docker boundary. Its mocks and synthetic browser
fixtures do not establish QEMU/KVM behavior. R1 release eligibility remains
closed until the remaining integration, isolation, takeover, recovery, and
performance evidence is recorded in #392.

On 2026-11-16, the process-limit stress subcase passed in the real-KVM
acceptance on this host with `pids.max=128`. During the seeded-defect VM run,
`pids.current` returned to its baseline of 67 after the bounded child attempted
additional worker threads; `pids.events max` increased from 0 to 1, proving the
kernel denied an excess task, and the child joined all successfully created
workers. The full acceptance passed in 128.29 seconds, recorded a 1,187,405,824
byte session memory peak, and recovered the killed QEMU process in 6 ms. This
is one local task-limit stress sample. The trusted-host workflow run
[43920061533](https://github.com/zbarno/kool.ade/actions/runs/43920061533)
passed its policy check but failed closed at runner inventory: GitHub returned
HTTP 403 to the workflow token, and the repository has no registered
self-hosted runner with the `computer-use-kvm` label.

On 2026-11-17, source including the bounded memory-pressure subcase passed the
full opt-in real-KVM acceptance on the same Linux x86_64 host. In the seeded
defect session, a separate child process joined the session cgroup and touched
anonymous pages until `memory.current` reached at least 85% of its 2 GiB hard
limit. The cgroup peak was 1,827,086,336 bytes, below the 2,147,483,648-byte
limit; `memory.events oom_kill` stayed at 0; and after the child exited,
`memory.current` returned from 1,189,720,064 to 1,189,126,144 bytes. The run
also detected the seeded QA defect, observed `pids.events max` increase 0→1,
recovered both the SIGKILLed controller and QEMU, and removed the session
resources. It passed in 138.65 seconds with 43.801-second provisioning,
89-millisecond screen observation, and 7-millisecond QEMU recovery. This
checks bounded memory pressure and release on one host; it does not test OOM
kill behavior, repeated or high-concurrency stress, or another host. The
acceptance remains opt-in and no hosted runner with the `computer-use-kvm`
label is registered.
