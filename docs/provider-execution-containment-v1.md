# Coding provider execution containment design

Design for issue
[#591](https://github.com/zbarno/kool.ade/issues/591): make Codex CLI and
Claude Code genuinely executable for eligible Kool.ad/e roles by establishing
an enforceable boundary around the provider process and **all of its
descendants**, so organization/account-managed hooks, helpers, plugins, MCP
helpers, and managed-policy-injected commands can no longer run with
controller-host authority. This document is implementation-plan step 1
(threat model + strategy evaluation) and is the contract later
implementation PRs must satisfy. It is deliberately normative about
boundaries and deliberately conservative about claims.

## Status of the current block

Today, execution is hard-denied for Codex and Claude regardless of machine
state:

- `src/harness/execution_security/profiles.rs` returns static block reasons
  (`CODEX_TASK_EXECUTION_BLOCK`, `CLAUDE_TASK_EXECUTION_BLOCK`) for every
  profile, so neither provider can produce profile eligibility evidence
  (`profile_evidence_id` refuses to mint IDs for blocked profiles).
- `src/harness/execution_security.rs` documents why: the provider CLI runs on
  the controller host and its cloud/account/organization-managed hook code
  cannot be inspected or suppressed ahead of launch. Feature-disable flags do
  not override managed policy, so the deny is unconditional.
- Partial, non-boundary mitigations already exist and stay as
  defense-in-depth:
  - `UNSAFE_PROVIDER_LAUNCH_ENV` scrubs environment variables that can
    inject startup code, redirect shells, retarget Git, or hijack IPC at
    provider launch (node/bun/python preload, git config overrides, VS Code
    IPC handles, `CLAUDE_CODE_SHELL_PREFIX`, ...). Provider auth/model
    transport variables are intentionally retained.
  - The app-owned MCP bridge and Docker `ExecutionSession`
    (`src/execution/session.rs`, `src/harness/execution_security/session_bridge.rs`)
    protect **brokered repository commands** — not the provider process
    tree, which is exactly the gap this design closes.

The block must not simply be removed. Requirement 10: hard-coded denials come
out only once the replacement enforces the same safety properties, and any
combination that cannot be contained keeps a precise, evidence-based
fail-closed reason instead of the current blanket one.

## Assets and adversaries

Assets, in decreasing blast radius:

| Asset | Notes |
| --- | --- |
| Controller filesystem, credentials (keychains, token stores), dotfiles | Owned by the interactive user |
| Docker daemon and socket, sibling task workspaces | Shared on a rootless-Docker host |
| Other host processes and privileged APIs | Anything the CLI tree could reach via execve/socket/filesystem |
| Task workspace clone and its outputs | Expected to be writable by the task — *inside* the boundary |
| Model-auth material presented to a provider endpoint | Necessary for the CLI to function at all |

Primary adversary: malicious or compromised **managed policy** —
organization-account hooks, plugin launch commands, skill/helper
instructions, MCP-helper invocations, or the provider binary itself —
acting with the privileges of whoever launches the CLI. Secondary:
untrusted third-party packages the CLI installs or executes inside a task.

The design invariant: **the provider container is the largest blast radius
any provider-originated code can ever have, including when the provider
binary or its managed config is hostile.** Everything outside that container
must remain unreachable.

## Threat enumeration

Vectors grouped by implementation-plan category. Each maps to a named
boundary below (M-mounts, E-env/secrets, N-network, P-process, D-daemon, T-task,
C-cancellation, S-secrecy-of-app-authority) and a confidence grade
(Enforced / Enforced-residual-risk / Prototype-required / Residual-disclosed).

| # | Vector | How it lands today (pre-fix) | Boundary | Grade |
| --- | --- | --- | --- | --- |
| 1 | Hook/managed-policy spawns host command at CLI startup | Runs as user; full FS/creds | N+M+E+P (runs only inside container) | Enforced |
| 2 | Plugin/extension install writes host paths (~, PATH dirs) | Arbitrary write | M (rootfs rw overlay discarded; no host mounts) | Enforced |
| 3 | CLI/helper reads host credential stores | File read | M (no host mounts; per-task scratch only) | Enforced |
| 4 | Exfiltrate secrets via unrestricted egress | Any TCP/TLS | N (allowlisted egress via app-controlled frontend only) | Enforced-residual-risk (DNS/time channels, §Network) |
| 5 | Reach Docker daemon, attach to/escalate from other containers | Socket/IPC | D (no docker.sock, no host PID/net ns, userns isolated rootless; no CAPs) | Enforced |
| 6 | Pivot via host filesystem symlinks/bind surprises | Mount-follow | M (fixed bind layout, resolved-before-start, refuse reparse/link classes on host layouts) | Enforced |
| 7 | Escape namespaces (setns, /proc tricks, CVE-class) | — | P (no-new-privs, seccomp filter on the rootless boundary, kernel-level risk disclosed) | Enforced-residual-risk |
| 8 | Inherit dangerous environment (preload/Git/VS Code IPC) | Env inheritance | E (deny-by-default env: injected set only; `UNSAFE_PROVIDER_LAUNCH_ENV` double-checked at container edge) | Enforced |
| 9 | Provider authenticates by loading host OAuth/token files | File read of ~/.codex, keychains | E+S (§Credential mediation: only the chosen strategy's minimum payload, scoped, revoked) | Enforced per-strategy; some strategies may remain fail-closed |
| 10 | Hook subprocess reads the model-auth payload | Same process tree as the CLI | E+T (disclosed, unavoidable: §Credential mediation; blast radius capped at container + scoped egress) | Residual-disclosed |
| 11 | Subprocess survives cancellation; mutates state after "stop" | Orphaned processes | C+P (single process-group/leader kill rooted at the container pid namespace; cancellation = container teardown) | Enforced |
| 12 | Stale "ready" evidence across upgrades/config drift | Cache staleness | T (§Capability evidence: versioned, hashed, invalidated on any input drift) | Enforced |
| 13 | Provider tries to drive app-owned MCP to approve/publish | Self-service escalation | S (app authorization is the final gate; MCP exposure inside the boundary is read/execute-as-Worker-token only; no Manager grant, no task-ownership writes, no publish/merge/deploy verbs) | Enforced |
| 14 | Two workspaces/tasks intermix via shared caches | FS aliases | T (per-task task-owned volumes; dependency cache by existing ref contracts, immutable) | Enforced |
| 15 | Windows/macOS equivalents diverge silently | Cross-platform skew | Platform gate: Linux/rootless-Docker only in v1; others keep precise unsupported diagnostics | Enforced |
| 16 | Manager receives hostile ambient instructions and acts | Prompt/instruction layer (orthogonal to OS containment) | Manager gate: §Manager eligibility keeps Codex/Claude Manager-blocked until instruction/config suppression is verifiably evidenced; containment alone never unlocks Manager | Enforced |

Residual-risk disclosures (honest ceiling of v1):
Kernel/namespace CVE-class escapes; covert timing/DNS side channels; and
credential visibility to the provider's *own* subtree (vector 10) — see
§Credential mediation for why v1 accepts it capped rather than fake-isolated.

## Strategy evaluation (requirement 1: evaluate before coding)

**Option A — Harden the host CLI in place** (more env scrubbing, HOME swap,
flag negotiation, config replacement, relying on app-owned MCP routing).
Rejected as *the* boundary: every element is spoofable or unmanageable by
managed policy (managed hooks ignore HOME swaps and flag negations), so the
residual "provider code runs as the user with the user's home" fails the
invariant. Parts become defense-in-depth *inside* Option B.

**Option B — Task-scoped, application-owned provider container**
(selected): run the provider CLI and its entire descendant tree in a
rootless-Docker container reusing the existing trusted infrastructure —
pinned composite images, `ExecutionSession` request/authorization/cancel
machinery, resource budgets, and telemetry attribution. The CLI talks to the
outside world only through the narrow front doors defined here.

**Option C — Standalone OS sandbox (a bespoke
unprivileged-namespace runner outside the Docker substrate, e.g.
systemd-nspawn profiles).** Rejected for v1: duplicates the Docker substrate the project
already trusts, loses pinning/composition/telemetry reuse, and fragments the
two execution authorities.

**Option D — Separate microVM per task.** Rejected for v1: latency and
footprint disproportionate; revisit if Option B's residual risks prove
insufficient in practice.

Selection rationale: requirement 2 (preserve Docker-only execution
authority, reuse `ExecutionSession`) points at B; B gives process,
filesystem, network, and lifecycle containment in one auditable unit.

## Target design (normative)

New unit: **ProviderRuntimeSession** — a thin specialization layered over
`ExecutionSession` concepts, owned by the harness, created per task/profile
pair, destroyed on completion/cancellation. Nothing in this design widens an
existing `ExecutionSession`; repository commands continue flowing through
today's app-owned MCP bridge untouched.

### Container construction

- **Engine**: rootless Docker via the existing engine abstraction
  (`src/execution/docker/*`); preflight rejects anything less than a
  verified rootless setup (unsupported daemon modes, missing user namespace
  requirements, corporate-managed daemons with incompatible profiles →
  precise diagnostics, no fallback to host execution).
- **Image**: application-pinned, versioned provider runtime images
  (one per provider × major version line), composed through the existing
  pinned-image path; image digest recorded in capability evidence and in
  session telemetry. No `latest`, no user images, no in-container package
  install for *provider* code (dependencies baked and hash-verified at
  compose time; task-level installs happen against the task workspace only
  and still stay in-container).
- **Identity/caps**: dedicated non-root container user; `no-new-privileges`;
  drop all capabilities; no device cgroup entries; seccomp default-filter
  tightened by a profile removing namespace-abuse syscalls where the engine
  permits; `pids.max` cap sized to task budget; memory/cpu/storage from the
  existing `ResourceBudgetV1` plumbing.

### Mounts (boundary M)

Fixed allow-set; computed once, resolved canonically before start, refused
on link/reparse anomalies (reusing the path-canonicalization discipline
built for the Windows backend work):

1. `<task>-workspace/:workspace` (rw) — the task clone, the only rw tree.
2. `<scratch>/cache:` rw tmpfs-like volume for provider caches, discarded.
3. Provider-neutral read-only assets dir (bundled, app-hash-verified) ro.
4. No host home. No `/etc/passwd`-adjacent user state. No docker socket,
   no `/var/run/docker.sock`, no host `/tmp`, no GPU/PCI passthrough in v1.
   No other binds, period.

### Process and environment (boundaries P, E)

- Deny-by-default environment: the launcher assembles the container env from
  an explicit allowlist (locale, TZ, workspace markers, mediator pointers).
  At the edge, `UNSAFE_PROVIDER_LAUNCH_ENV` is re-asserted defensively and
  violation aborts launch (tests assert this catch fires).
- Single init: the container pid 1 is a minimal supervisor owned by the app
  (signal forwarding, leader tracking). The provider CLI is its supervised
  child. **Cancellation = tear down the container**; pid-namespace kill is
  the mechanism, not a best-effort process walk. Teardown is logged and
  idempotent; liveness of stray provider descendants after cancel is an
  asserted-tested property.

### Network (boundary N)

- No default bridge egress. The provider container sits on a per-session
  internal network with exactly two exits, both app-controlled:
  1. **Model-egress front door**: app-owned proxy endpoint reachable only
     from the container network; it is a forward proxy that enforces an endpoint allowlist (per-provider model
     API hostsets), strips hop-by-hop and credential headers it owns, and
     logs/attributes connections. Private/CGNAT/link-local/metadata-address
     destinations are hard-refused at the proxy and at the container's
     firewall layer (defense in depth; the proxy is not the only wall).
     This is the mechanism carrying §Credential mediation.
  2. **Package/tool front door (optional, off by default)**: same proxy,
     separate allowlist profile; enabled per provider need, metered, logged.
- DNS served only by the app-side frontend for allowlisted zones; direct
  DNS egress blocked. Protocols: HTTPS 443 only (no raw-socket categories in
  v1); everything else unroutable.
- v1 residual (disclosed, test-watched): DNS amplification/covert-timing
  channels through the proxy; ICMP; and clock-skew timing — bounded,
  measured, and enumerated in release notes; none reaches host state.

Engineering spike (early phase 2): rootless-Docker per-session networks +
iptables/nftables egress filtering with slirp4netns variance across
distros; the spike's measured behavior feeds the proxy/firewall contract.

### Credential mediation (boundary E + S; requirement 4, told straight)

Honest constraint: the provider CLI must authenticate to its model endpoint.
Whatever credential form the chosen strategy uses (saved OAuth profile
material or API-key auth), **the provider process tree can see it by
construction** — any hook code is in that tree. v1 therefore makes the
containment promise instead of a fake-isolation claim:

- Credentials never exist on the host in a form the container could read:
  host-side storage stays in keychain/secure-store; the controller loads the
  minimum payload at session start.
- Preferred transport shapes, strongest first:
  1. **Proxy-held credentials**: API-key strategy where the *front door*
     signs requests — the container sees no bearer; only works where the
     provider CLI accepts proxied/basic-less auth or custom transport
     settings; if Codex/Claude cannot be pointed at the proxy without the
     literal key/env in-container, fall to (2).
  2. **Injected, scoped, revocable, in-container only**: the chosen
     strategy's credential material enters the container environment at
     start and nowhere else; not written to any mounted volume; stripped
     from logs/telemetry by the redactor; lifetime = session; destruction =
     container teardown (credential cells wiped with the scratch).
- Saved-OAuth *refresh* flows that demand interactive host browsers or host
  state are **fail-closed in v1** (reported as "auth strategy X not
  mediatable; re-auth required" with the exact blocker) rather than
  approximated. No strategy may "look" safe while leaking wider material
  (no wholesale home mounts, no exported token bundles beyond the minimum).
- The disclosure is carried in capability evidence (§below) so users and
  operators see the exact auth class and its blast-radius contract.

### App authority (boundary S; requirement 6)

Inside the boundary, provider-initiated calls to app-owned surfaces use a
Worker-class token issued by the controller: execute curated repository
operations through the existing app-owned MCP path, nothing more. Nonexistent
in v1: task-ownership mutation, security-setting writes, route/routing
selection, publish/merge/deploy verbs, consent shortcuts. Provider or hook
output can propose; only the controller's existing authorization chain can
authorize. Tests assert every escalated verb is refused with its own
reason.

### Capability evidence (boundary T; requirement 8)

Versioned, invalidatable ladder replacing the static block constants:

```text
provider_state      : undetected | detected | authenticated | transport_ready
                      | worker_contained | manager_eligible | unavailable
capability_kind     : provider_version | binary_path_hash | image_digest
                      | auth_strategy_class | transport_config
                      | container_probe | egress_probe | mcp_authority_probe
                      | instruction_isolation_probe (Manager only)
invalidation_inputs : provider version, resolved path + content hash,
                      runtime image digest, auth mode/source class,
                      execution profile, engine/build identity,
                      egress-front-door policy fingerprint
```

Rules: every transition is evidence-bearing (each kind records input
digests/hashes and a checked-at generation); any drift in
`invalidation_inputs` drops the cached state to the lowest known rung and
forces re-probe — **no stale green**; `worker_contained` requires the full
v1 probe battery (process kill, mount inventory, egress negative tests,
env-edge tripwire, MCP-authority refusals) to pass against the *live*
container shape; evidence is surfaced in UI/routing reports as the
plain-English ladder ("detected / authenticated / contained / Manager-ready
/ unavailable: <exact reason>"). Detection/auth run on host as today (the
CLI binary itself being examined is inert); the container probes run in a
disposable probe container, never a real task.

## Manager vs Worker (requirements 5; ties to #451)

- **Worker** unlocked when `worker_contained` holds for that exact
  combination (provider×version×auth×engine×policy fingerprint). Worker
  retains provider-native features whose effects stay inside the verified
  boundary (in-container installs, task-file operations); anything needing
  host effect is not a Worker feature.
- **Manager stays blocked for Codex/Claude in v1**, with the *updated*
  evidence-based reason (instruction/config suppression not verifiable),
  distinct from "cannot contain the process." Containment of the process
  tree does not contain the instruction layer: a hostile organization can
  still feed a Manager-hostile agent perfectly legal in-container behavior.
  `manager_eligible` therefore requires an additional, separately evidenced
  `instruction_isolation_probe` (effective instruction/config resolution
  demonstrated locked to app-owned content) — out of v1 scope for these two
  providers, so Manager routing for them remains evidence-blocked. Existing
  Manager blocks for antigravity/opencode/copilot keep their reasons and are
  unaffected. Unblocking Worker never flips Manager (regression-asserted).

## Rollout and implementation phases

Maps to the issue's implementation plan; each phase lands as its own
reviewable PR against this document, with the corresponding evidence rung
turning on only inside that PR.

1. **Phase 1 (this document)**: threat model + strategy; merged design.
   Phase-2 status: the preflight evaluator (PR #714), the
   `ProviderRuntimeSession` foundation (PRs #715/#716: sealed immutable
   specs with fixed budgets, prefix-named containers, digest-only pin
   validation including canonical lock-form acceptance, host-mount denial,
   tmpfs allowlist, the executor port, and fail-closed staged teardown gated
   on verified-absent observations), and the probe-battery catalog (typed
   16-vector threat matrix mirroring the table above, with
   hermetic/contained/manual-evidence execution classes and a provably inert
   structural runner) are all merged. The `containers/provider-runtime/`
   recipe remains an intentional unpinned placeholder per its documented
   promotion flow. Still in phase 2: publishing/pinning the real runtime
   image (ops action), wiring probe EXECUTION against a contained daemon,
   and daemon-real teardown E2E.
2. **Phase 2 — containment foundation**: `ProviderRuntimeSession` skeleton;
   preflight for rootless Docker adequacy; pinned provider runtime image
   recipes (empty-shell first); probe battery scaffolding; teardown/kill
   guarantees with negative tests (mount inventory, escape attempts: docker
   socket probes, `/proc` walks, env tripwire, symlink follows).
3. **Phase 2a — network spike**: measure slirp/userns variance; freeze the
   egress-proxy + firewall contract; negative-egress test harness (metadata
   IPs, CGNAT, raw ports, DNS).
4. **Phase 3 — transport/auth**: front-doors operational; auth-strategy
   mediators (API-key classes first); refresh-flow fail-closed reporting;
   redaction assertions in logs/telemetry.
5. **Phase 4 — Codex integration**: probe evidence wired into
   `profiles.rs` replacements (dynamic reasons; block constant retired for
   `worker` when `worker_contained`); capability UI/report ladder; adversarial
   fixtures (managed-hook-style in-container payloads attempting vectors 1–9).
6. **Phase 5 — Claude Code integration**: same contract; managed-settings/hook
   fixtures; sign-in/transport equivalence tests.
7. **Phase 6 — Manager adjudication + readiness**: per-#451 Manager
   evaluation for each provider (expected outcome v1: documented
   continued-Manager-block with the new evidence class); setup/discovery/
   reports updated; docs/README support-matrix accuracy pass.
8. **Phase 7 — regression/security matrix**: adversarial end-to-end suite per
   plan step 8; real-provider smoke evidence recorded *separately* from
   fixture claims, fed into integrated release validation (#144/#156
   streams) — never counted as fixture-passed.

Platform scope for v1: **local Linux with the existing rootless Docker
path** (architecture requirement 7). macOS/WSL/remote workers remain
fail-closed with their existing precise diagnostics until equivalent
boundaries exist.

## Out of scope (kept out on purpose)

Man.ager redesign; task state machine changes; ACP migration; enabling
unrestricted host tools; granting arbitrary network; relaxing
provider-behavior policy broadly; Windows/macOS containment variants;
microVM substrate; any claim that fixture testing equals release
certification.

## References

- `src/harness/execution_security/profiles.rs` — current block mechanics
  (constants, `profile_block_reason`, `profile_evidence_id`).
- `src/harness/execution_security.rs` — host-side transport limitation,
  `UNSAFE_PROVIDER_LAUNCH_ENV`, boundary/module layout.
- `src/harness/execution_security/session_bridge.rs` — app-owned MCP bridge
  over Docker `ExecutionSession` (repository-command protection; unchanged
  by this design).
- `src/execution/session.rs` — `ExecutionSession` request/authorization,
  image digest, cancellation generations, resource budgets, runtime
  processes.
- `src/execution/docker/*` — engine abstraction, pinned/composed images.
- `docs/coding-tools.md`, `docs/machine-bootstrap.md` — current user-facing
  statements of the disabled state (updated in phase 6).
- Related: #451 (Manager/Worker profiles), #94, #136, #144, #146, #156.
