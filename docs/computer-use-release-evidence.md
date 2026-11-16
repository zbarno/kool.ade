# Computer Use cross-tier release evidence

**Status recorded:** 2026-11-16. **Release state:** not certified. Issue
[#392](https://github.com/zbarno/kool.ade/issues/392) and epic
[#375](https://github.com/zbarno/kool.ade/issues/375) remain open. A merged
component issue or a green synthetic test does not establish a released
Computer Use capability.

This is the cross-tier evidence index for #392. It distinguishes merged
implementation, automated checks, real-host evidence, and release enablement.
Only evidence linked below has been observed; pending or missing evidence is
not a pass.

The Welcome setup ability matrix reads the same shared R2–R5 release gates as
runtime enforcement. Verified setup tasks therefore cannot advertise Internet
Research or Personal mode while their release gates are closed; Sensitive
external actions and Remote computer workers are also shown as unavailable.
Regression coverage exercises fully verified setup evidence against those
closed gates.

## Evidence matrix

| Capability | Implementation and automated evidence | Real-host / release evidence | Current gate and remaining proof |
| --- | --- | --- | --- |
| R1 offline disposable VM | Signed generation 19 is catalog-pinned after [PR #628](https://github.com/zbarno/kool.ade/pull/628); bounded app-helper capacity is in [PR #627](https://github.com/zbarno/kool.ade/pull/627). The merged real-KVM suite checks live block/PCI/network/chardev graphs (including QMP denial of an injected NIC on the non-hotpluggable offline bus), QEMU environment isolation, effective cgroup resource ceilings, completed GUI actions, unsupported-accessibility denial, staged-app launch through the P15 bridge, a seeded QA-driver assertion, teardown cleanup, and performance instrumentation in [PRs #632](https://github.com/zbarno/kool.ade/pull/632), [#644](https://github.com/zbarno/kool.ade/pull/644), [#646](https://github.com/zbarno/kool.ade/pull/646), [#647](https://github.com/zbarno/kool.ade/pull/647), [#648](https://github.com/zbarno/kool.ade/pull/648), [#649](https://github.com/zbarno/kool.ade/pull/649), [#650](https://github.com/zbarno/kool.ade/pull/650), [#651](https://github.com/zbarno/kool.ade/pull/651), [#652](https://github.com/zbarno/kool.ade/pull/652), [#653](https://github.com/zbarno/kool.ade/pull/653), [#659](https://github.com/zbarno/kool.ade/pull/659), [#661](https://github.com/zbarno/kool.ade/pull/661), and [#662](https://github.com/zbarno/kool.ade/pull/662); their hosted checks passed. PR #660 added conservative fail-closed regression budgets for provisioning (180s), screen observation (2s), idle memory (1.5 GiB), idle CPU (2.5 CPU-seconds per idle second), peak memory (1.75 GiB of the 2 GiB cgroup bound), and cleanup (10s). These are test regression budgets, not product SLOs or multi-host stress evidence. | A trusted local Linux x86_64 host passed boot, authenticated Chromium launch, guest/display capture, mouse/keyboard actions, takeover/handback, live cgroup limit assertions, an approved staged QA GUI app, the production Computer QA scenario driver, and cleanup against signed generation 19 (`sha256:53ca225f40a0229085cf8da26b8e205b94052679fc39e0903a6999e6dee706ec`). The VM scenario recorded a seeded wrong-destination assertion failure with a real guest screenshot. | Published candidate, signed catalog promotion, and generation 19 release passed. Same-host measurements across reported runs recorded provisioning at 37.277–46.443s, observation at 54–128ms, idle memory at 1.072–1.112GB, peak cgroup memory at 1.110–1.206GB, and cleanup at 117–147ms. The recorded runs check one network hotplug-denial path and component-level transfer denials for symlink ancestors/destinations, empty/oversize files, FIFOs, tar traversal, and hardlinks. They do not prove full guest/host escape or transport-level adversarial denial. Two fresh signed-generation-19 VMs on a trusted local Linux x86_64 host exercised seeded-defect detection and corrected-build success, guest/display frames, takeover/handback, and QA cleanup. Both ran in separate controller processes: after the full journey the controller was SIGKILLed while QEMU remained alive, then recovery in the parent process found one matching event and removed QEMU, the session directory, cgroup, and overlay. The full seeded and corrected acceptance runs took 136.40s and 132.75s; controller restart recovery took 134ms for each and QEMU crash recovery took 7ms for each. Viewport geometry has deterministic sizing and coordinate-mapping tests, but live viewer usability, memory-pressure and concurrent-session exhaustion, and hosted/multi-host KVM evidence remain incomplete. A bounded real-KVM `pids.max=128` stress sample did make the kernel deny an excess thread (`pids.events max` 0→1) and returned `pids.current` to baseline (67) after joining workers; this is one task-limit sample, not broad resource-exhaustion coverage. R1 is not certified. |
| Host readiness and novice setup (#378) | The readiness and setup implementation is merged; preflight checks Linux architecture, QEMU/KVM access, root-owned QEMU binaries, and delegated cgroup v2 controllers. | Pop!_OS 24.04 LTS, x86_64, kernel `7.2.15-76070215-generic`, Intel Core i7-1195G7, QEMU/qemu-img 8.2.2, writable `/dev/kvm`; the host preflight and real guest acceptance both passed in a transient delegated user scope. | This proves readiness and the positive boot/action/stream/cleanup path on this host. Novice setup usability and broader isolation/negative tests remain separate evidence. |
| Guest image and authenticated companion (#379, #380, #381) | Generation 19 is the current published signed catalog entry. It includes the merged app-helper cgroup capacity fix and companion protocol 1. | The local real-KVM acceptance verified the signed generation 19 image, authenticated the companion, launched Chromium, captured authenticated guest and display frames, and released the VM successfully. | Candidate build [43557367751](https://github.com/zbarno/kool.ade/actions/runs/43557367751), promotion [43558652176](https://github.com/zbarno/kool.ade/actions/runs/43558652176), catalog PR [#628](https://github.com/zbarno/kool.ade/pull/628), and publication [43561711152](https://github.com/zbarno/kool.ade/actions/runs/43561711152) completed. Broader guest/host isolation negatives and guest OS restart semantics remain unverified; local QEMU and controller process recovery passed, but no hosted KVM runner is registered. |
| GUI actions, viewer/takeover, guest apps, artifacts, and QA repair (#382, #383, #385, #389, #394) | The component issues are merged with automated contract, policy, artifact, provisioning, and UI coverage. The Task Details viewer has deterministic responsive sizing and pointer-coordinate mapping tests in [PR #657](https://github.com/zbarno/kool.ade/pull/657). The Computer VM Runtime QA driver has a seeded wrong-destination regression: a typed desktop scenario records a failed assertion with screenshot evidence, and a corrected synthetic re-test records passing assertions in [PR #658](https://github.com/zbarno/kool.ade/pull/658). [PR #662](https://github.com/zbarno/kool.ade/pull/662) adds adversarial component tests for file-transfer boundaries. The guest reports accessibility-tree inspection as unsupported in v1. | Real KVM accepted mouse/keyboard actions against Chromium, verified changed guest frames, and exercised actor replacement and stale-authority rejection during human control and handback. [PR #653](https://github.com/zbarno/kool.ade/pull/653) adds a live denial check after takeover/handback: the guest advertises accessibility as unsupported, the host read returns `UnsupportedCapability`, and the action journal remains `NotRecorded`. [PR #659](https://github.com/zbarno/kool.ade/pull/659) runs a seeded wrong-destination QA assertion through the production scenario driver in a real guest and exercises takeover/handback in the same session. | Component-level file-bridge adversarial cases are now covered, but they have not been replayed against the live guest transport. Real-VM seeded-defect and corrected-build QA runs both pass their expected assertions. QEMU and controller process crash cleanup are now covered on the local host; live viewer UI takeover usability and guest OS restart semantics remain unverified. |
| R2 public Internet research (#386) | The restricted research network profile and proxy boundary are implemented and covered by policy/component tests. The controller and Welcome ability matrix share the closed R2 release gate. | No real-VM outbound-network, metadata, private-destination, or proxy-bypass matrix is recorded in #392. | R2 remains unavailable until real-VM allowed-destination, metadata/private-address denial, pause/revoke, and proxy-bypass checks pass and are recorded. |
| R3 encrypted personal VM and synthetic authentication (#387) | The implementation is merged. The synthetic IdP/MFA/browser-restart job passed in candidate run 43189760047. Runtime startup and the Welcome ability matrix share the closed R3 release gate. | No personal persistent VM was accepted on a real host. | Keep personal mode unavailable until its threat-model and real-host evidence are accepted. |
| R4 sensitive external actions (#388) | The policy validator rejects `HighImpactExternalAction` through the shared closed R4 gate until exact transaction grants exist; denial and stale-approval tests are present. | No checkout, publication, or other external side-effect flow is certified. No real money or real accounts are used in CI. | High-impact external actions remain unsupported. A future gate needs synthetic-account allow/deny, changed-detail, duplicate-execution, and uncertain-outcome evidence. |
| R5 remote VM workers (#390) | Implementation issue #390 closed after [PR #572](https://github.com/zbarno/kool.ade/pull/572) and separate-process worker fixture coverage in [PR #634](https://github.com/zbarno/kool.ade/pull/634) merged; the fixtures reject wrong host fingerprints and cross-worker sessions. | These fixtures use separate local processes and in-process stdio streams. They are not real SSH connections or separate physical hosts. | The shared R5 gate, also used by `remote_computer_use_release_gate_open_v1()`, remains closed; #392 tracks separate-host SSH, worker restart/interruption, and competing-operator evidence. Implementation closure does not certify or enable remote-worker support. |
| macOS and Windows host adapters (#395) | [PR #638](https://github.com/zbarno/kool.ade/pull/638) merged fail-closed, architecture-specific launch planning. [PR #641](https://github.com/zbarno/kool.ade/pull/641) merged exact Windows AF_UNIX storage-leaf validation; [PR #642](https://github.com/zbarno/kool.ade/pull/642) merged the app-owned listener, current-QEMU peer PID verification, and a gated Windows client-mode launch candidate. Required Windows-native tests and hosted package/image checks passed. The Linux baseline prerequisite was accepted on 2026-11-14 and refreshed on 2026-11-15. | No macOS Intel, macOS Apple silicon, or Windows x64 host VM evidence is recorded. | These PRs do not provide a verified Windows QEMU client build, full VM process/display/input lifecycle, platform disk/network isolation, setup UX, or host certification. Both hosts remain unsupported until their adapters, deterministic security checks, and independent real-host evidence pass the #392 release gate. |

## Reproducibility record

The current published signed catalog is generation 19,
`computer-use-images-v1-19-53ca225f40a0`, based on
`06baa2475c72d06270a08758326a9af7c17bd856`. The image digest is
`sha256:53ca225f40a0229085cf8da26b8e205b94052679fc39e0903a6999e6dee706ec`,
the companion digest is
`sha256:0f71003e35be1007e2b236a3cb728ef87946bbcd1be10963e13cece15ee272fa`,
and the signed catalog manifest digest is
`sha256:76a1828c8b0fde2978e9b32c83d2ba840a67ae95ab6a5ef8f30ef5666b17d5c4`.
The catalog update merged in [PR #628](https://github.com/zbarno/kool.ade/pull/628)
as `2fac33867ca018d601ee2f93ed219bde9fc106d2` and was published by
[run 43561711152](https://github.com/zbarno/kool.ade/actions/runs/43561711152).

On 2026-11-14, the local host passed the production real-KVM acceptance test
against that published digest. The host was Pop!_OS 24.04 LTS, Linux x86_64,
kernel `7.2.15-76070215-generic`, 11th Gen Intel Core i7-1195G7, QEMU and
qemu-img `8.2.2`; `systemd-detect-virt --vm` reported `none`, and `/dev/kvm`
was writable. The test ran under
`systemd-run --user --scope --property=Delegate=yes`, downloaded and verified
the 682,426,368-byte image, booted the disposable VM, authenticated companion
protocol 1, launched Chromium, received valid 1280×800 guest and QEMU display
frames, and completed VM release. With
`KOOLADE_COMPUTER_USE_R1_REAL_KVM=1` and
`KOOLADE_R1_REAL_KVM_HOST_SECRET_CANARY=synthetic-canary` set in the delegated
systemd scope, `cargo +1.98.1 test --locked --lib
computer_use_r1_real_kvm_acceptance -- --nocapture` completed with
`1 passed; 0 failed` in 945.19 seconds total; this duration
includes image download, so it is not a provisioning-latency measurement.
The evidence is recorded in [#392 comment 6872727893](https://github.com/zbarno/kool.ade/issues/392#issuecomment-6872727893).

This is positive boot, authenticated launch, observation, display, and release
evidence for one Linux host and one image. It is not evidence for adversarial
guest/host isolation, input actions, file transfer, takeover, crash recovery,
other hosts, or feature tiers that remain disabled. No online Actions runner
labeled `computer-use-kvm` is registered; this acceptance was run locally.

At source commit `dbdf4828bf4a63801189e483bc1b04e644b86c39`, the same opt-in
acceptance test also passed a cached-image guest interaction/handoff run in
47.44 seconds. It used the same host and published image, exercised a click,
Ctrl+L, typed local data URL and Enter, refreshed observations after each input,
and performed guest keyboard input under HumanControl before handing authority
back to the agent. The test verified the screen digest changed and that revoked
agent/human capabilities could not execute after replacement. This checks the
controller/guest action and actor-fencing path; it does not test the live viewer
UI, external manual input, file bridge, or guest/host escape boundaries.

At PR #632 head `e24e2dcaf5e68b0bc490b233be4837e0fd7fdb8b`, the same local host
passed the opt-in real-KVM test in 68.95 seconds with the published generation
19 image. QMP confirmed the disabled-network VM exposed no PCI network device;
the QEMU process did not inherit the synthetic runner canary; and a synthetic
P15 grant and artifact crossed the authenticated guest channel and reached the
guest-helper launch handoff. The command ran in a delegated systemd user scope
with `KOOLADE_COMPUTER_USE_R1_REAL_KVM=1` and
`KOOLADE_R1_REAL_KVM_HOST_SECRET_CANARY=synthetic-canary`. The artifact uses
synthetic approval data and proves transfer/protocol acceptance only, not real
QA approval or application readiness. PR #632 merged to `main` at
`8c6e44a2bb9f1ce9a75094cf0f60681d786a89c9` after its hosted checks passed.

On 2026-11-15, the host preflight passed in a transient delegated user scope
and the same opt-in real-KVM acceptance passed against the tree merged in
[PR #638](https://github.com/zbarno/kool.ade/pull/638) as
`d562fb1d31a81bf81686f89959091de35f93ffeb`. The run used the published
generation 19 image (`sha256:53ca225f40a0229085cf8da26b8e205b94052679fc39e0903a6999e6dee706ec`),
companion protocol 1, the synthetic host-secret canary, and completed in
50.06 seconds with `1 passed; 0 failed`. It produced valid 1280x800 guest and
display frames and released the VM. The command was
`systemd-run --user --scope --property=Delegate=yes --setenv=KOOLADE_COMPUTER_USE_R1_REAL_KVM=1 --setenv=KOOLADE_R1_REAL_KVM_HOST_SECRET_CANARY=synthetic-canary cargo +1.98.1 test --locked --lib computer_use_r1_real_kvm_acceptance -- --nocapture`.
This remains one positive Linux-host run and does not close the outstanding
adversarial isolation, recovery, usability, performance, or other-platform
gates.

After [PR #644](https://github.com/zbarno/kool.ade/pull/644) merged at
`c24c2d37ff37ffb503f494332340a656611b6998`, the same trusted Linux x86_64
host ran the opt-in acceptance three more times against the published
generation 19 image. All three runs passed: 51.41, 51.10, and 50.14 seconds.
Each run confirmed that release removed its session runtime directory and
delegated cgroup and left no session overlay in the temporary overlay
directory. This establishes repeatability for the cleanup assertion on one
host; it is not a resource-exhaustion stress test or a multi-host result.

On 2026-11-15, a follow-up to this evidence made the real-KVM interaction
assertions fail closed on non-completed guest replies. It moves the one-second
idle sample before the interaction frame is captured, then requires the guest
to accept the mouse click, Ctrl+L shortcut, typed local data URL, and Enter
key; the screenshot digest must change. An initial run exposed that the older
ordering reused a stale frame after the idle sample and the test did not check
the guest's `StaleObservation` reply. The corrected acceptance passed on the
same Linux KVM host in 52.52 seconds against the published generation 19
image. This verifies that those input actions complete on the real guest; it
does not establish the complete seeded-defect QA repair/rebuild journey or
live viewer usability.

At the follow-up chardev-isolation revision, the same host passed an additional
live QMP `query-chardev` check against the published generation 19 guest. The
check requires the QMP and HMP monitors and authenticated guest-control socket
to use their exact session-local Unix socket paths, permits the test-only
diagnostic serial endpoint, and rejects missing or additional character
devices. The real-KVM acceptance passed in 48.71 seconds; both the exact
allowlist and an injected unapproved host endpoint passed their focused parser
tests. This does not complete the broader guest/host isolation matrix.

Subsequent merged real-KVM checks expanded the live-device and guest-authority
evidence. [PR #647](https://github.com/zbarno/kool.ade/pull/647) verifies with
QMP that the sole block device uses the approved signed base image and that
session's disposable overlay. [PR #648](https://github.com/zbarno/kool.ade/pull/648)
rejects PCI devices outside the explicit allowlist. [PR #649](https://github.com/zbarno/kool.ade/pull/649)
requires each Chromium mouse/keyboard action to return `Completed` before the
test accepts a changed screenshot. [PR #650](https://github.com/zbarno/kool.ade/pull/650)
checks the live QMP/HMP/guest-control chardev graph, while [PR #651](https://github.com/zbarno/kool.ade/pull/651)
preserves duplicates so unapproved duplicate endpoints cannot be hidden.
[PR #652](https://github.com/zbarno/kool.ade/pull/652) confirms the live QEMU
process environment contains exactly `PATH`, `HOME`, `LC_ALL`, and the
session-specific `TMPDIR`. Finally, [PR #653](https://github.com/zbarno/kool.ade/pull/653)
checks that accessibility remains unavailable after human takeover and agent
handback and that its denied read has no journal side effect. PR #653 merged as
`2155261456b53636d7eadc1c7b95e38f45559142`; its hosted tests, docs, Clippy,
CodeQL, recovery, image, and native-package checks passed. These checks narrow
the matrix but do not establish host escape resistance, filesystem/network
adversarial denial, or complete end-to-end QA.

On 2026-11-16, the opt-in acceptance added a read of the live kernel cgroup
controls after QEMU boot. The cached-image run passed with `cpu.max` set to
`200000 100000` (2 CPU-equivalents), `memory.max` set to 2 GiB,
`memory.swap.max=0`, `memory.oom.group=1`, and `pids.max=128`; the live task
count was nonzero and within the ceiling. The GUI check now allows up to one
second for the authenticated guest frame to reflect completed input actions.
This verifies configured kernel bounds on one host, not resistance to resource
exhaustion or multi-host behavior.

PR #646 added opt-in instrumentation for image verification/acquisition,
provisioning, observation latency, idle CPU/memory, peak cgroup memory, and
cleanup. Three consecutive same-host measurements recorded provisioning at
37.277–37.491 seconds, screen observation at 113–128 ms, idle memory at
1.072–1.087 GB, peak cgroup memory at 1.110–1.143 GB, and cleanup at
118–120 ms. These are observational measurements from one host, not a
resource-exhaustion stress result.

The P21 performance follow-up adds fail-closed regression budgets to the
opt-in real-KVM acceptance: provisioning 180 seconds, screen observation
2 seconds, idle memory 1.5 GiB, idle CPU 2.5 CPU-seconds per second of idle
sampling, peak memory 1.75 GiB under the 2 GiB cgroup limit, and cleanup
10 seconds. Each budget has a focused test proving that an over-budget metric
fails. The budgets are deliberately conservative regression guards, not
product latency guarantees or a substitute for repeated stress and multi-host
measurements. On 2026-11-16, the local host passed these checks against signed
generation 19 in 70.41 seconds: provisioning 44.739 seconds, observation
83 ms, idle memory 1,086,316,544 bytes, idle CPU 1,878,958 usec over a
1,000 ms sample, peak memory 1,206,063,104 bytes, and cleanup 147 ms. No
hosted KVM runner result is claimed.

On 2026-11-16, the same host also ran the opt-in KVM acceptance with an
approved native GUI QA fixture staged through the real P15 transfer bridge and
launched by the authenticated guest helper. The production
`ComputerQaScenarioDriverV1` waited for the new guest window, clicked the
fixture through authenticated guest input, captured the real display, and
recorded the seeded wrong-destination assertion as failed. The screenshot
digest was `d75a31b0b73225a1d14eabf418d84897f1f27e422d0437405cf61afeb5036a16`.
The full KVM acceptance passed in 73.33 seconds; VM provisioning was 46.443
seconds, observation was 68 ms, session peak memory was 1,205,698,560 bytes,
and cleanup was 146 ms. This run also covered human control during task-app
launch, guest action, and agent handback. The fixture is synthetic, has no
network access, and does not perform an application rebuild or corrected
retest; the corrected rerun remains covered by the synthetic driver test in
PR #658. The workflow remains manually dispatched from protected `main`; no
hosted runner result is claimed for this local run.

A subsequent local opt-in KVM run attempted to add a virtio network device
connected only to an isolated QEMU hubport, with no host network backend. QEMU
rejected the `device_add` because the offline guest PCIe root bus does not
support hotplug; the temporary netdev was removed and the live PCI/network
graphs still matched the approved configuration. This confirms the configured
offline bus denies this network-device injection path. It does not prove
resistance to every hypervisor escape or guest network bypass. The complete
acceptance passed in 61.18 seconds, including the existing Computer QA seeded
failure and performance budgets; provisioning was 38.938 seconds, screen
observation 54 ms, idle memory 1,088,249,856 bytes, idle CPU 1,616,951 usec
over 1,000 ms, peak memory 1,191,317,504 bytes, and cleanup 117 ms. This is
another single-host local run; no hosted KVM runner result is claimed.

On 2026-11-16, the latest merged `main` revision at the time of this record
(`9442941d828575353020fcb7320259fd94a5fdcd`) passed the trusted-host preflight
and the complete opt-in KVM acceptance in a delegated user scope. The host was
Pop!_OS 24.04 LTS, Linux x86_64, kernel `7.2.15-76070215-generic`, with QEMU
and qemu-img 8.2.2, writable `/dev/kvm`, and delegated cgroup v2 controllers.
The signed generation 19 guest booted, authenticated companion protocol 1,
launched Chromium, captured 1280×800 guest and display frames, staged and
launched the approved QA fixture, recorded the seeded wrong-destination
assertion as failed, exercised takeover/handback, and cleaned up. The test
passed in 67.98 seconds: provisioning 42.603s, observation 68ms, idle memory
1,083,334,656 bytes, idle CPU 1,842,635 usec over 1,000ms, peak memory
1,190,768,640 bytes, cleanup 143ms. The screenshot digest was
`d75a31b0b73225a1d14eabf418d84897f1f27e422d0437405cf61afeb5036a16`. This is
an additional local single-host run; it does not supply a hosted KVM-runner
result, corrected real-VM rebuild/retest, or broad escape/stress evidence.

On 2026-11-16, the seeded-failure and corrected-build QA paths each passed in
their own fresh disposable VM session on the same host and signed generation
19 image. Separate sessions are required because guest staging permits one
approved app launch per session. The seeded fixture produced the expected
failed destination assertion and screenshot digest
`d75a31b0b73225a1d14eabf418d84897f1f27e422d0437405cf61afeb5036a16`; the
corrected fixture passed and produced screenshot digest
`69986be27f63ebf60374fa22d1a002bd55afd3ab4b5913c631d4d760dd688221`. Both
runs authenticated the guest, captured 1280×800 guest/display frames, and
cleaned up (149 ms and 144 ms). Provisioning took 42.973 seconds for the seeded
run and 42.746 seconds for the corrected run. This is a single-host local
result and does not replace hosted KVM execution, broad escape/stress
coverage, or the live viewer and recovery gates.

Both fresh-session QA runs also injected an unexpected QEMU process crash after
the GUI/QA and takeover checks. The live backend collected the stopped process
with no normal exit code, then a fresh backend instance recovered exactly one
matching session event and removed the session directory, cgroup, and overlay.
Recovery and cleanup took 7 ms for the seeded-defect run and 10 ms for the
corrected-build run. This verifies QEMU process crash cleanup and durable
session recovery within the same controller process; it does not exercise a
controller process restart or repeated resource-exhaustion stress.

The same opt-in KVM acceptance then exercised an actual controller process
restart for both the seeded defect and corrected build. A child process owned
the live session through the QA and takeover journey, was terminated with
SIGKILL while its QEMU process remained alive, and a fresh process recovered
one matching event and removed QEMU, the session directory, cgroup, and
overlay. The full seeded-defect acceptance completed in 136.40 seconds and the
corrected-build run in 132.75 seconds. Controller restart recovery took
134 ms in each run; separate QEMU crash recovery took 7 ms in each run. This
remains single-host evidence; no hosted KVM runner is registered.

The official real-KVM workflow is manually dispatched on protected `main` and
requires that runner label. A run with `require_kvm_host=false` is explicitly
not acceptance evidence. The owner-operated usability walkthrough is not a
closure gate; the automated real-host security and release checks above still
are.

## Release controls and gaps

- R3 personal persistent sessions remain disabled by
  `PERSONAL_VM_R3_ENABLED = false`.
- R4 high-impact external actions are rejected by policy until exact
  transaction grants exist.
- Remote VM scheduling remains disabled by
  `remote_computer_use_release_gate_open_v1() = false`.
- macOS Intel, macOS Apple Silicon, and Windows x64 host adapters remain unsupported until their independent #392 release gates pass.
- Accessibility-tree inspection is unsupported in the guest v1 agent.
- No broad adversarial guest/host isolation matrix or resource-exhaustion
  stress run is recorded. Three repeated cleanup runs and one controller
  restart per seeded/corrected path passed on one host; these do not cover
  broad escape attempts or repeated resource-exhaustion cases.
- Owner-operated walkthrough and manual review are independent of issue
  closure; neither is represented as automated evidence or required for the
  implementation status recorded here.

Until those gaps are addressed and the corresponding issue evidence is
updated, #392 and #375 stay open and unsupported tiers remain fail-closed.

A bounded process-limit stress check was added to the opt-in real-KVM suite.
On 2026-11-16, both seeded-defect and corrected-build VM runs reached
`pids.max=128`, observed the kernel denial counter rise from 0 to 1, and
returned the cgroup task count to baseline (67 and 42) after joining all
workers. The full runs passed in 128.29s and 127.39s with peak session memory
of 1,187,405,824 and 1,174,798,336 bytes, and 6 ms QEMU crash recovery in both.
This establishes one real-host process-limit enforcement sample. Repeated,
concurrent-session, memory-pressure, and multi-host exhaustion remain untested;
no hosted `computer-use-kvm` runner is registered.
