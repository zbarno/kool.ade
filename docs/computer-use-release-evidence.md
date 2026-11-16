# Computer Use cross-tier release evidence

**Status recorded:** 2026-11-15. **Release state:** not certified. Issue
[#392](https://github.com/zbarno/kool.ade/issues/392) and epic
[#375](https://github.com/zbarno/kool.ade/issues/375) remain open. A merged
component issue or a green synthetic test does not establish a released
Computer Use capability.

This is the cross-tier evidence index for #392. It distinguishes merged
implementation, automated checks, real-host evidence, and release enablement.
Only evidence linked below has been observed; pending or missing evidence is
not a pass.

## Evidence matrix

| Capability | Implementation and automated evidence | Real-host / release evidence | Current gate and remaining proof |
| --- | --- | --- | --- |
| R1 offline disposable VM | Signed generation 19 is catalog-pinned after [PR #628](https://github.com/zbarno/kool.ade/pull/628); bounded app-helper capacity is in [PR #627](https://github.com/zbarno/kool.ade/pull/627). The merged real-KVM suite checks live block/PCI/network/chardev graphs, QEMU environment isolation, effective cgroup resource ceilings, completed GUI actions, unsupported-accessibility denial, artifact handoff, teardown cleanup, and performance instrumentation in [PRs #632](https://github.com/zbarno/kool.ade/pull/632), [#644](https://github.com/zbarno/kool.ade/pull/644), [#646](https://github.com/zbarno/kool.ade/pull/646), [#647](https://github.com/zbarno/kool.ade/pull/647), [#648](https://github.com/zbarno/kool.ade/pull/648), [#649](https://github.com/zbarno/kool.ade/pull/649), [#650](https://github.com/zbarno/kool.ade/pull/650), [#651](https://github.com/zbarno/kool.ade/pull/651), [#652](https://github.com/zbarno/kool.ade/pull/652), and [#653](https://github.com/zbarno/kool.ade/pull/653); their hosted checks passed. | A trusted local Linux x86_64 host passed boot, authenticated Chromium launch, guest/display capture, mouse/keyboard actions, takeover/handback, live cgroup limit assertions, and cleanup against signed generation 19 (`sha256:53ca225f40a0229085cf8da26b8e205b94052679fc39e0903a6999e6dee706ec`). Three repeated cleanup runs passed. PR #653 merged at `2155261456b53636d7eadc1c7b95e38f45559142`. | Published candidate, signed catalog promotion, and generation 19 release passed. Same-host measurements recorded provisioning at 37.277–40.521s, observation at 61–128ms, idle memory at 1.072–1.112GB, peak cgroup memory at 1.110–1.156GB, and cleanup at 118–121ms. The new run verified `cpu.max=200000 100000`, `memory.max=2147483648`, `memory.swap.max=0`, `memory.oom.group=1`, and `pids.max=128`, with live tasks within that ceiling. These observations have no agreed performance thresholds and are not stress evidence. Full adversarial host/guest/network isolation, seeded QA repair/rebuild, file-bridge adversarial cases, live viewer UI, crash recovery, and stress/performance acceptance remain incomplete. R1 is not certified. |
| Host readiness and novice setup (#378) | The readiness and setup implementation is merged; preflight checks Linux architecture, QEMU/KVM access, root-owned QEMU binaries, and delegated cgroup v2 controllers. | Pop!_OS 24.04 LTS, x86_64, kernel `7.2.15-76070215-generic`, Intel Core i7-1195G7, QEMU/qemu-img 8.2.2, writable `/dev/kvm`; the host preflight and real guest acceptance both passed in a transient delegated user scope. | This proves readiness and the positive boot/action/stream/cleanup path on this host. Novice setup usability and broader isolation/negative tests remain separate evidence. |
| Guest image and authenticated companion (#379, #380, #381) | Generation 19 is the current published signed catalog entry. It includes the merged app-helper cgroup capacity fix and companion protocol 1. | The local real-KVM acceptance verified the signed generation 19 image, authenticated the companion, launched Chromium, captured authenticated guest and display frames, and released the VM successfully. | Candidate build [43557367751](https://github.com/zbarno/kool.ade/actions/runs/43557367751), promotion [43558652176](https://github.com/zbarno/kool.ade/actions/runs/43558652176), catalog PR [#628](https://github.com/zbarno/kool.ade/pull/628), and publication [43561711152](https://github.com/zbarno/kool.ade/actions/runs/43561711152) completed. Broader guest/host isolation negatives and restart/crash recovery remain unverified. |
| GUI actions, viewer/takeover, guest apps, artifacts, and QA repair (#382, #383, #385, #389, #394) | The component issues are merged with automated contract, policy, artifact, provisioning, and UI coverage. The Task Details viewer now has deterministic responsive sizing and pointer-coordinate mapping tests in [PR #657](https://github.com/zbarno/kool.ade/pull/657). The guest reports accessibility-tree inspection as unsupported in v1. | On 2026-11-14, real KVM accepted mouse/keyboard actions against Chromium, verified changed guest frames, and exercised actor replacement and stale-authority rejection during human control and handback. [PR #653](https://github.com/zbarno/kool.ade/pull/653) adds a live denial check after takeover/handback: the guest advertises accessibility as unsupported, the host read returns `UnsupportedCapability`, and the action journal remains `NotRecorded`. | File bridge, artifact isolation under adversarial input, seeded QA repair, live viewer UI takeover usability, integrated viewport/usability, and performance-threshold acceptance still need evidence. Mock and synthetic-browser tests do not prove these paths. |
| R2 public Internet research (#386) | The restricted research network profile and proxy boundary are implemented and covered by policy/component tests. The controller does not offer Internet Research while `INTERNET_RESEARCH_R2_RELEASE_ENABLED_V1` is false. | No real-VM outbound-network, metadata, private-destination, or proxy-bypass matrix is recorded in #392. | R2 remains unavailable until real-VM allowed-destination, metadata/private-address denial, pause/revoke, and proxy-bypass checks pass and are recorded. |
| R3 encrypted personal VM and synthetic authentication (#387) | The implementation is merged. The synthetic IdP/MFA/browser-restart job passed in candidate run 43189760047. | No personal persistent VM was accepted on a real host. | `PERSONAL_VM_R3_ENABLED` is `false`; keep personal mode unavailable until its threat-model and real-host evidence are accepted. |
| R4 sensitive external actions (#388) | The policy validator rejects `HighImpactExternalAction` until exact transaction grants exist; denial and stale-approval tests are present. | No checkout, publication, or other external side-effect flow is certified. No real money or real accounts are used in CI. | High-impact external actions remain unsupported. A future gate needs synthetic-account allow/deny, changed-detail, duplicate-execution, and uncertain-outcome evidence. |
| R5 remote VM workers (#390) | Implementation issue #390 closed after [PR #572](https://github.com/zbarno/kool.ade/pull/572) and separate-process worker fixture coverage in [PR #634](https://github.com/zbarno/kool.ade/pull/634) merged; the fixtures reject wrong host fingerprints and cross-worker sessions. | These fixtures use separate local processes and in-process stdio streams. They are not real SSH connections or separate physical hosts. | `remote_computer_use_release_gate_open_v1()` remains `false`; #392 tracks separate-host SSH, worker restart/interruption, and competing-operator evidence. Implementation closure does not certify or enable remote-worker support. |
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
exhaustion or accepted performance thresholds.

PR #646 added opt-in instrumentation for image verification/acquisition,
provisioning, observation latency, idle CPU/memory, peak cgroup memory, and
cleanup. Three consecutive same-host measurements recorded provisioning at
37.277–37.491 seconds, screen observation at 113–128 ms, idle memory at
1.072–1.087 GB, peak cgroup memory at 1.110–1.143 GB, and cleanup at
118–120 ms. These are observational measurements from one host, not agreed
performance thresholds or a resource-exhaustion stress result.

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
- No broad adversarial guest/host isolation matrix, resource-exhaustion stress
  run, or accepted provisioning/screen/idle performance measurements are
  recorded yet. Three repeated cleanup runs passed on one host; the successful
  positive-path runs do not cover these remaining cases.
- At merge time, PRs #593 and #595 were merged by `zbarno`; GitHub had no
  submitted formal review on either PR. The owner may review manually, but no
  independent GitHub review event is asserted here.

Until those gaps are addressed and the corresponding issue evidence is
updated, #392 and #375 stay open and unsupported tiers remain fail-closed.
