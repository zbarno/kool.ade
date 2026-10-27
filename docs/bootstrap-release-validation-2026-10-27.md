# Bootstrap release validation record — 2026-10-27

Tracking: [#337](https://github.com/zbarno/kool.ade/issues/337) and
[#351](https://github.com/zbarno/kool.ade/issues/351).

Captured 2026-10-27 at 10:09 UTC. This is a dated evidence record, not a
release approval. The checks below
distinguish implementation and synthetic-test evidence from native-host
acceptance. No platform becomes supported because a test was skipped or could
not run.

## Snapshot identity

| Item | Value |
| --- | --- |
| Main baseline | `dfd130aa4446e606684117b7ef913ea9db01ca18` |
| macOS setup PR | [#403](https://github.com/zbarno/kool.ade/pull/403), head `1017f777243237a5a0e3ba22a7318a0d31d2b4ca` |
| Windows setup | [#349](https://github.com/zbarno/kool.ade/issues/349), probes in progress; no accepted PR at this snapshot |
| Image lock | `containers/images.lock.json`, set `2026-10-13.2`, file SHA-256 `7faecd1593642d6f8bbe874a0607dec270fce2fe080f553513cb76108971af77` |
| General image digests | `linux/amd64` Debian `sha256:a4672c0cb26fbdde88e38fa2dfb6c681942306680e41e4378b28770b6e79ee91`; `linux/arm64` Debian `sha256:a1b86db52ce3daef089e45aabe36dfec4091f82464c25c1fdcf03de197cbe82a` |
| Reviewer approval | Pending external PR review; no release reviewer approval recorded |

The general image lock also includes Rust, Node, Python, and Go variants for
`linux/amd64` and `linux/arm64`. The browser and desktop QA profiles remain
`runtime_enabled: false`; their candidate digests do not enable runtime QA.

### Status update — 2026-10-27 11:24 UTC

PR [#403](https://github.com/zbarno/kool.ade/pull/403) merged at
`8928689bd1116ccab617564b4001e2373ccc5be9`. Windows readiness implementation
is in ready-for-review PR [#404](https://github.com/zbarno/kool.ade/pull/404).
For this update, main is `244bb22538dd2e677cd58f31e5aa77ef8029a227` and the
#404 head is `a236796cd5e12d9f0438cd3b164fac0c09472c72`. On that exact #404
head, local format, 3,406-test Nextest (8 skipped), doc tests, strict Clippy,
and Windows x64 GNU library cross-check passed. Hosted checks were running at
the time of this update. These checks do not replace native Windows 11 x64
validation listed below. The platform matrix and release decision remain
unchanged: native acceptance and independent release approval are still
required.

### Status update — 2026-10-27 12:33 UTC

PR [#404](https://github.com/zbarno/kool.ade/pull/404), head
`a236796cd5e12d9f0438cd3b164fac0c09472c72`, merged to main at
`528133b6fa1e47e9c10861099ec6907ccbab50c3`. Its hosted check matrix passed.
This adds Windows 11 x64 readiness implementation; native Windows acceptance
and the Linux, macOS, security, and usability validations in this record remain
pending. The merged code and synthetic tests do not enable Windows support or
unblock the release.

## Candidate host matrix — snapshot as of 2026-10-27 10:09 UTC

The README's Linux x86_64 experimental host status is distinct from acceptance
of the specific rootless bootstrap recipes below. These are #351 validation
cells, not new support claims.

| Host target | Current implementation evidence | Native acceptance | Release status |
| --- | --- | --- | --- |
| Linux x86_64 current experimental host | Existing experimental application path; this record does not change product availability | Full first-run personas, rootless bootstrap and lifecycle/security evidence below | Existing experimental status only; #351 release acceptance remains open |
| Ubuntu 24.04 amd64, rootless Docker | Fixed recipe and typed host checks in the bootstrap implementation | Disposable VM install/elevation, existing rootful daemon coexistence, reboot/relogin, task flow and security checks | Unverified; blocked |
| Debian 13 amd64, rootless Docker | Fixed recipe and typed host checks in the bootstrap implementation | Same Linux acceptance on a disposable VM | Unverified; blocked |
| Fedora 45 amd64, rootless Docker | Fixed DNF5 recipe and typed host checks in the bootstrap implementation | Same Linux acceptance on a disposable VM | Unverified; blocked |
| Pop!_OS 24.04 amd64, rootless Docker | Fixed recipe and archive checks in the bootstrap implementation | Same Linux acceptance on a disposable VM | Unverified; blocked |
| Linux arm64 host | Unsigned native CI package only; runtime image variants do not establish host support | Separate native rootless acceptance per distro | Unsupported; not advertised |
| macOS Intel, Docker Desktop Linux VM | [#403](https://github.com/zbarno/kool.ade/pull/403) adds app publisher, release, architecture, and setup evidence; local Apple Silicon cross-compilation passed | Confirm current official bundle identifier and signer; signed Desktop smoke and isolated task/security run on Intel | Unverified; blocked |
| macOS Apple silicon, Docker Desktop Linux VM | [#403](https://github.com/zbarno/kool.ade/pull/403) adds app publisher, release, physical architecture, and setup evidence | Confirm current official bundle identifier and signer; signed Desktop smoke and isolated task/security run on Apple silicon | Unverified; blocked |
| Windows 11 x64, WSL2 and Docker Desktop | At the 10:09 snapshot, #349 probes were in progress; readiness implementation later merged in [#404](https://github.com/zbarno/kool.ade/pull/404) | Native edition/build, feature, kernel, reboot/policy, signed Desktop, task and security validation | Unverified; blocked |
| Windows 10 x64, Windows ARM64, unlisted Linux distributions, older or unsupported macOS/Windows releases | No accepted support evidence | Separate native acceptance is required before claiming Windows 10 or ARM64 support | Unsupported or unknown |

Record the exact macOS and Windows versions/builds for each native run. Docker
Desktop's [Mac support policy](https://docs.docker.com/desktop/setup/install/mac-install/)
is a vendor prerequisite, not proof that Kool.ad/e has validated those releases.

Docker Desktop host free space is not VM capacity evidence. Rosetta/emulation,
synthetic fixtures, cross-compilation, and CI-provisioned Docker do not count as
native support proof. Unknown capacity, publisher, endpoint, kernel, or policy
facts must remain blocked.

## Required user and task walkthroughs

No signed results or nonexpert usability report are attached to this record.
Run each walkthrough without prior task-flow knowledge and record the host,
build, exact task IDs, result, and observations.

- [ ] No coding CLI and no Docker; get to deterministic setup guidance.
- [ ] CLI installed but unauthenticated, stale authentication, and missing
  model; each state produces a separate actionable task without silent
  provider switching.
- [ ] Correct CLI already selected; confirm the ready path remains stable.
- [ ] No Docker or no approved image; install/prepare guidance remains separate
  and image pull requires explicit approval.
- [ ] Linux rootful daemon or wrong-user socket; verify no rootful fallback.
- [ ] macOS Docker Desktop stopped, first-run/license prompt, and permission
  failure; setup remains Finder-first and does not bypass system controls.
- [ ] Windows WSL2 or virtualization disabled and Windows-container mode;
  each state remains distinct and recoverable.
- [ ] Multiple simultaneous faults, stable task IDs across workspaces,
  dependency edges, per-step help, approval preview, cancellation, retry,
  rollback, crash/reboot recovery, independent recheck, and return-to-work.
- [ ] Novice completion without terminal commands except separately approved
  OS elevation, reboot, or documented enterprise-policy constraints.
- [ ] Keyboard, screen-reader, reduced-motion, and 360/900/1280/1600 px review.

## Required security and real-task evidence

The standard Linux suite on the #403 head passed locally: 3,383 tests passed,
8 skipped; doc tests (0), format, and strict Clippy passed. An independent code
review found no remaining code-level validator blocker. These results do not
replace native host or complete lifecycle validation. Final hosted check
conclusions and external review must be recorded before any release.

Hosted workflow snapshot for [PR #403](https://github.com/zbarno/kool.ade/pull/403):

| Workflow/job | Run | Snapshot status |
| --- | --- | --- |
| [Koolade quality gates](https://github.com/zbarno/kool.ade/actions/runs/40481811940) | `40481811940` | Format, Clippy, documentation tests, implementation tests, Docker-only runtime, daemon recovery, queue E2E, Windows discovery, and Docker runtime all passed |
| [Native desktop packages](https://github.com/zbarno/kool.ade/actions/runs/40481811849) | `40481811849` | Linux amd64/arm64, macOS Intel/Apple silicon, and Windows unsigned packages all passed |
| [Trusted executor images](https://github.com/zbarno/kool.ade/actions/runs/40481811820) | `40481811820` | Browser/desktop builds, published image smokes, and reviewed image digest check passed; publication and attestation were skipped |
| [CodeQL](https://github.com/zbarno/kool.ade/actions/runs/40481809656) | `40481809656` | Actions, JavaScript/TypeScript, Python, and Rust analysis passed |

The CodeQL aggregator run `122113967044` passed. The skipped publication and
attestation jobs do not enable runtime QA; both QA profiles remain disabled.

- [ ] Red-team malformed or user-controlled remediation parameters, unexpected
  executables, provider-text prompt injection, secret-bearing diagnostics, and
  cancellation/race conditions on each supported host.
- [ ] Spoofed Docker contexts, Unix sockets and Windows named pipes; symlink
  and reparse-point cases; wrong image architecture/digest; stale evidence.
- [ ] Confirm endpoint identity is bound to the effective UI/launcher and no
  rootful or remote fallback is selected.
- [ ] Run context-only bootstrap guidance, then a Docker-backed isolated code
  read/change and validated synchronization.
- [ ] Prove network, credentials, host files, arbitrary host commands, and
  unapproved tools are denied; record role authorization, CPU/memory/PID
  quotas, and container cleanup.
- [ ] Attach raw security checklist, signed installer attestation/signature
  evidence where applicable, CI job/run links, actual OS/device/architecture
  versions, and a short nonexpert usability report.

## Release decision

**Release blocked.** Native acceptance is not recorded for the Linux targets,
either Mac architecture, or Windows 11 WSL2. The current official Docker
Desktop signer requirement also needs confirmation against the released Intel
and Apple silicon application bundles. Keep browser and desktop runtime QA
disabled until #171 passes. Preserve the production-release security conclusion
from #144; onboarding tests do not replace it. Do not enable, advertise, or
release any platform cell until its evidence is reviewed and this record is
updated with the reviewer, exact run, branch SHA, and image digests.
