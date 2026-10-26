# Machine bootstrap readiness and setup guidance

Tracking: [#337](https://github.com/zbarno/kool.ade/issues/337).

## Implementation status: incremental foundation; provider routeability and locked image preparation remain in progress

The readiness catalog, machine-local tasks, shared setup UI, remediation
broker, and provider setup guidance are implemented in slices #339–#343. No
fresh-install path currently proves a provider can authenticate, expose a
usable model, and run in Kool.ad/e's context-only route. Issue #343 remains
open for that routeability gap. Native execution and novice first-run
acceptance also remain tracked by #346–#351.

Kool.ad/e starts bounded discovery of the six supported coding-agent CLIs and
Git from first launch, before a workspace or Docker daemon exists. Provider
readiness requires a current executable and version, authenticated account,
fresh model evidence, and safe context-only capability. Unknown authentication
or model access fails closed. An unavailable optional provider does not make a
selected provider unusable, and Kool.ad/e never silently selects another
provider.

## Readiness data and task lifecycle

`MachineSnapshotV1` is a versioned, typed snapshot. It records per-platform
capability facts, the state and source generation of each probe, and a
dependency graph that distinguishes ready, missing, unknown, stale, failed,
unsupported, blocked, and not-applicable results. Unknown or stale evidence is
never promoted to ready. Docker images are reported missing only after a
trusted, queryable engine reports its image inventory; a stopped, rootful,
inaccessible, or untrusted engine blocks image inspection instead.

Findings reconcile into a machine-local task collection under operator state,
outside project repositories. The versioned store migrates prior schemas,
including generic Docker and provider findings that must be rechecked before
replacement. It reserves a durable scan generation, uses atomic writes and an
inter-process lock, and bounds task, attempt, and event history. A validated
dependency graph gives stable task order, detects cycles, and projects
unresolved prerequisites as blockers. Fresh, domain-matched probe evidence is
required to verify a task; a regression reopens it. Restart recovery interrupts
in-flight work and clears active authorization bindings, so operations and
approvals are never replayed automatically.

The scan keeps provider output, credentials, model names, executable paths,
Docker endpoint paths, and daemon identifiers out of the persisted snapshot.
Endpoint and daemon identity fingerprints stay in memory to reject a result
when the selected runtime changes. A final bounded identity check and local
commit are serialized with refresh requests. Canceled or stale scan
generations are discarded. A saved observation or authorization record never
authorizes a host mutation or replaces an execution boundary.

## Welcome and task UI

The Welcome screen shows a machine-setup board before a workspace is connected.
Each active finding has its own card with its current state, prerequisites,
guidance, a rescan control, and a repair action only where a typed repair is
supported. Kanban uses the same task cards and opens the shared Task Details
shell. Details provide plain-language guidance beside structured status,
blockers, progress history, and verification evidence; narrow windows stack the
panes. Platform-specific vendor guides are linked from each task. Unsupported
platforms remain explicitly unavailable and never appear ready.

Image preparation opens a review first. Approval sends a typed application
command through the consent gate; it does not grant setup guidance access to a
host shell. The setup view shows only bounded status and redacted audit
metadata. Raw host command output and credentials are not retained. Attempt and
authorization records are typed audit metadata only; they do not execute repairs,
grant host access, or authorize machine mutation.

## Provider setup task priority (#343)

Coding Tools offers a distinct “Prioritize this provider for setup tasks” choice
alongside “Default tool.” It selects the provider Kool.ad/e can use for guided
setup; when the Default tool is a different provider, its own readiness tasks
remain visible as well. It does not route conversations: Default tool and
work-type routes control conversation routing, with Manager remaining
authoritative. If no guided-setup provider is selected, the explicitly chosen
Default tool is used as the setup candidate. Discovery never chooses Pi or
another provider automatically. The machine setup board shows one provider
chooser before a choice and creates provider-specific tasks after a choice.

Provider task details link to the provider's official installation, sign-in,
and model documentation as appropriate for the task. Sign-in tasks include
provider-specific browser, device-code, or CLI steps where documented. The
operator completes those steps in the provider's own terminal or browser;
Kool.ad/e does not launch provider sign-in or handle passwords, API keys,
one-time codes, OAuth tokens, or browser data. After a manual change, the
operator retries discovery; only fresh provider evidence can resolve the
corresponding task.

The broker offers one reviewed provider installation recipe: Claude Code
v2.1.310 on Ubuntu 24.04 x86_64. It uses fixed Anthropic release URLs, checks
the signed release manifest against Anthropic's pinned signing-key fingerprint
and checks the binary SHA-256. The review names the source, publisher, release,
file effects, and current-user privilege. Installation writes only
`~/.local/bin/claude`; it does not run as root or change system packages. Other
Claude hosts and the other five providers use their official installation
guidance until a host-specific recipe has its own reviewed trust policy.
Anthropic documents the signed manifest and Linux binary verification in its
[Claude Code setup guide](https://code.claude.com/docs/en/setup).
Kool.ad/e can still run Claude's bounded version and sign-in status probes
when no detectable local managed host policy blocks them; these probes do not
start a task session. Claude Code task execution from Kool.ad/e is disabled
until a process sandbox can contain organization-managed hooks.
Anthropic documents that managed settings can define hooks and that
[managed hooks](https://code.claude.com/docs/en/hooks) cannot be disabled by
user or project settings. The CLI's restricted mode and inline settings cannot
guarantee that account-managed hooks will stay inside Kool.ad/e's Docker
boundary. Choose another provider for Kool.ad/e tasks while this restriction
is in place. Claude installation and sign-in guidance remain available.

Install-channel guidance is conservative: only the reviewed Claude Code signed
binary for Ubuntu 24.04 x86_64 has an app-owned recipe. apt-like, rpm-like,
Arch-like, Alpine, macOS Homebrew/pkg, Windows winget/MSI, and unknown or
mismatched distro combinations remain vendor-guide/manual-only or unsupported.
Kool.ad/e does not infer install commands from a detected package manager.

Authentication and account enrollment remain on the provider's own surface.
After installation or sign-in, Kool.ad/e runs a fresh provider probe. A
provider without a safe non-generative account check remains unverified, even
if local credentials or model names are present.

Authentication evidence is provider-specific. Pi's `auth check` resolves local
credentials rather than testing account access. Pi also allows `auth.json` to
resolve a key by running a configured `!command`; Kool.ad/e does not invoke this
check and leaves Pi authentication unknown. Its bundled/configured model
catalog can be cached or used offline. See Pi's [provider guide](https://pi.dev/docs/latest/providers)
for command-backed keys and [CLI reference](https://pi.dev/docs/latest/cli) for
the credential command. Stored OpenCode
credentials, Copilot sign-in configuration, and Antigravity configuration/model
listings also do not prove current account access and remain unverified. Claude
model names read from local configuration are not treated as a fresh provider
catalog. Claude Code's documented
[`auth status` command](https://code.claude.com/docs/en/cli-reference)
records that the CLI reports a logged-in method; it does not prove current
model entitlement. Providers without a safe
non-generative login check retain an explicit authentication-verification task
and cannot become ready as a setup agent until the required evidence is fresh.

Codex task execution from Kool.ad/e is disabled until a process sandbox can
contain organization-managed hooks. Codex's `allow_managed_hooks_only` setting
still allows administrator-managed hooks, and managed policy can require hook
features; Kool.ad/e's local launch flags cannot rule out those host-side
commands. The [Codex configuration reference](https://github.com/openai/codex/blob/main/docs/config.md)
documents this behavior. Codex installation and sign-in guidance remain
available, but use another provider for Kool.ad/e task assistance until the
execution boundary can contain managed hooks.

OpenCode's `models` output is catalog/configuration evidence. A local model
without credentials is considered usable only when a bounded, non-generative
`GET /v1/models` check succeeds against its explicitly configured loopback HTTP
endpoint and lists that model. IPv4/IPv6 literals are direct; the exact hostname
`localhost` maps to `127.0.0.1` without DNS. The check sends no credentials,
does not follow redirects or proxies, and treats unsupported endpoints or an
unreachable service as stale model evidence. For Ollama, LM Studio, and vLLM,
authentication ready means credentials are not required; it does not prove the
endpoint or model is available. A live endpoint that omits the configured model
produces a fresh catalog with no usable model. Remote model lists and saved
remote credentials do not verify account access and remain unverified.

## Locked image preparation

Locked image tasks are created only after a trusted native Linux rootless Engine
is available. Their IDs bind the runtime profile, Engine architecture, and a
stable hash of the endpoint and daemon identity. General is always checked;
Rust, Node, Python, and Go are requested from actual workload requirements.
Preparation selects only the bundled digest lock, shows its registry and
approximate transfer and storage costs, then pulls only after the operator
approves. A successful task records an Engine- and digest-bound receipt after
the isolated smoke test. Workload execution rechecks that receipt and inspects
the exact local image; ordinary execution never pulls an image implicitly.
Browser and desktop QA images remain disabled. Docker Desktop image preparation
remains gated on the trusted VM transport and capacity evidence tracked by #347.

## Safety contract

- A Docker or image finding comes only from its own typed probe evidence. An
  unavailable, stopped, rootful, or untrusted daemon does not prove an image is
  missing.
- Machine setup records live under operator-local app state. They are not added
  to `.koolade-packet`, project planning Git, task clones, or shared team state.
- Task identity is stable across workspaces on one machine. A new scan cannot
  duplicate an existing prerequisite or overwrite a newer observation.
- A verified state requires fresh authoritative evidence for that task's
  readiness domain. A model response, unrelated failure, or stale scan is not
  evidence of success.
- Persisted approval metadata does not execute an operation. Remediation must
  use a typed action, genuine user consent, any required native privilege
  prompt, and an independent readiness probe.
- Provider setup-task priority only determines which provider receives
  provider-specific setup tasks; it does not select a conversation route.
  Default tool and work-type routes control conversation routing, with Manager
  remaining authoritative. Setup guidance cannot invent or execute host shell
  commands, elevate itself, or override Docker rootless policy. Deterministic
  setup guidance remains available when no coding CLI is installed or
  authenticated.
- Runtime QA remains disabled until #171 passes.

The #342 remediation broker now defines strict typed requests and exact plans,
derives the current host identity and OS version at consent and launch, and
persists authorization, bounded redacted outcomes, and post-operation probe
receipts in the setup journal. A launched operation is probed after success,
failure, timeout, or cancellation. Restart recovery marks an in-flight repair
interrupted and requires fresh setup evidence before any retry.

This provider slice registers one production operation: a current-user
Claude Code install limited to Ubuntu 24.04 x86_64. It is bound to the exact
reviewed release, source, target, and host predicate; the controller requires
current consent and rechecks the target before installation. It does not
elevate or change system packages. Existing consented Linux rootless image
preparation remains the #362 path. Other distro-specific Linux, macOS, and
Windows host operations belong to #346, #348, and #349. The general native
elevation executor remains fail-closed, and its supervised command runner is
disabled on macOS until its process-containment adapter is implemented. This
slice makes no new native execution platform claim.

## Platform and operation limits

The capability matrix marks Ubuntu 24.04 x86_64 as the current experimental
Linux controller combination. It records Linux family and numeric major and
minor version using a fixed enum; unrecognized distribution IDs collapse to
`other`. The probe checks subordinate UID/GID mappings, the current systemd
user socket, and known package-manager executable locations without storing
account names or raw distribution identifiers. Other Linux releases,
distributions, and architectures remain unverified. Rootless readiness also
requires a verified rootless Engine, supported cgroups, capacity, and security
evidence; systemd-user availability alone does not establish execution
support.

Windows WSL2, virtualization, and Docker Desktop trust facts that have not been
directly probed remain unknown. macOS and Windows native execution remains
blocked pending native Docker Desktop staging, security, and runtime
validation. Synthetic platform fixtures validate classification; they do not
establish support on those hosts.

This readiness layer does not install packages, configure services, pull images
without explicit approval, or execute model-provided shell commands. A supported
remediation operation uses a typed action, required operator approval and
native privilege confirmation, and an independent readiness probe. If a coding
CLI is absent or unusable, deterministic setup instructions remain available
without agent assistance.

## Acceptance evidence and remaining work

The automated suite covers typed evidence and dependency consistency,
redaction, stale generations, one-provider task selection, exact image
prerequisites, task-store lifecycle, and the UI result-acceptance boundary.
Human validation is still required for
novice first-run usability, visual review at 360/900/1280/1600 px, keyboard and
screen-reader use, reduced motion, and real-host setup behavior. Release
acceptance also requires disposable native Linux checks for rootless Docker and
Git/provider readiness; macOS Docker Desktop on Intel and Apple Silicon;
Windows WSL2 x64; signed native packaging; and the full security/usability gate
tracked by #342–#351. Mocks or Linux CI do not certify those platforms.

Native package build artifacts are unsigned and CI-only. They do not change the
support matrix; see [native desktop packaging status](native-desktop-distribution.md).
