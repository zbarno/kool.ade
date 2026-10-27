# Machine bootstrap readiness and setup guidance

Tracking: [#337](https://github.com/zbarno/kool.ade/issues/337).

## Implementation status: guided provider setup, Linux rootless and desktop onboarding in progress

The readiness catalog, machine-local tasks, shared setup UI, remediation
broker, and provider setup guidance are implemented in slices #339–#343.
OpenCode v2 with a saved Anthropic API key now has a non-generative account
check and fresh usable-model evidence. Actual provider conversation, Docker
MCP routing, accessibility, and native first-run behavior still need human
validation; PR #374 records that follow-up. OAuth and environment credentials
remain unverified. Linux rootless repair recipes are implemented for the exact
supported amd64 distributions, with disposable-VM acceptance still required.
Native execution and novice first-run follow-ups are tracked by #346–#351.

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

Image preparation and supported host repairs open a review first. Approval
sends a typed application command through the consent gate; it does not give the
setup assistant host-shell access. Linux rootless repair uses fixed package,
subordinate-ID, and current-user service operations, with native elevation only
for package and subordinate-ID changes. The setup view shows bounded status
and redacted audit metadata. Raw host command output and credentials are not
retained. Attempt and authorization records are typed audit metadata only; they
do not execute repairs, grant host access, or authorize machine mutation.

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
the credential command. For OpenCode v2, a saved Anthropic API key can be
checked without generating a response: Kool.ad/e captures only the selected
provider's documented `auth export` output in memory, sends one fixed HTTPS
`GET /v1/models` request to Anthropic, and intersects active API model IDs with
OpenCode's selectable model catalog. It does not follow redirects or use a
proxy, and it never records the exported key in setup-task logs. OAuth and
environment credentials, custom Anthropic provider settings, and OpenCode
versions without the provider-scoped export remain unverified. OpenCode's
[provider guide](https://dev.opencode.ai/v2/docs/cli/providers) and
[CLI commands](https://dev.opencode.ai/v2/docs/cli/commands/) describe login and
credential export; Anthropic documents the non-generative
[List Models API](https://platform.claude.com/docs/en/api/models/list).
Copilot sign-in configuration and Antigravity configuration/model listings do
not prove current account access and remain unverified. Claude model names read
from local configuration are not treated as a fresh provider catalog. Claude
Code's documented
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
credentials remain unverified unless they use the OpenCode v2 Anthropic API-key
check described above. Verified API keys are re-exported only for the selected
Anthropic model into a private per-invocation home for provider execution.

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

The #342 remediation broker defines strict typed requests and exact plans,
derives the current host identity and OS version at consent and launch, and
persists authorization, bounded redacted outcomes, and post-operation probe
receipts in the setup journal. A launched operation is probed after success,
failure, timeout, or cancellation. Restart recovery marks an in-flight repair
interrupted and requires fresh setup evidence before any retry.

The #343 provider slice registers one production operation: a current-user
Claude Code install limited to Ubuntu 24.04 x86_64. It is bound to the exact
reviewed release, source, target, and host predicate; the controller requires
current consent and rechecks the target before installation. It does not
elevate or change system packages. Existing consented Linux rootless image
preparation remains the #362 path.

The #346 controller registers narrowly scoped Debian, Ubuntu, Pop!_OS, and
Fedora rootless package, subordinate-ID, and current-user systemd-service
operations for exact amd64 recipes. Every mutation has an exact typed preview
and user consent; package and subordinate-ID operations also require the native
administrator prompt. The supervisor rechecks the selected package source or
account mappings immediately before elevation and independently probes
readiness after execution. A failed or interrupted operation is not replayed
automatically. The general native elevation executor remains fail-closed, and
its supervised command runner is disabled on macOS until its process-containment
adapter is implemented. This work makes no new macOS or Windows native
execution claim. The separate #362 image-preparation flow still requires an
already reachable, verified Engine; other macOS and Windows host operations
remain follow-ups under #348 and #349.

## Platform and operation limits

The experimental Linux matrix is Debian 13 amd64, Ubuntu 24.04 amd64,
Pop!_OS 24.04 amd64, and Fedora 45 x86_64. Debian, Ubuntu, and Pop!_OS use
exact APT sources; Fedora uses only the exact DNF5 recipe. Other releases,
other architectures, and other RPM-family systems remain manual. The app never
infers a recipe from the presence of `apt-get` or `dnf` alone. These recipes
still require disposable-VM acceptance before they can be called validated.
The upstream package references are Docker's [Debian](https://docs.docker.com/engine/install/debian/),
[Ubuntu](https://docs.docker.com/engine/install/ubuntu/), and
[Fedora](https://docs.docker.com/engine/install/fedora/) guides. Docker does
not officially support Ubuntu derivative distributions; Pop!_OS therefore
remains an experimental Noble compatibility recipe until its VM acceptance
passes.

The read-only host probe distinguishes missing, inaccessible, malformed, and
observed facts. Automatic rootless repair requires kernel 4.15 or later, the
documented minimum for Docker cgroup v2 support (see [Docker's cgroup v2
requirements](https://docs.docker.com/engine/containers/runmetrics/#running-docker-on-cgroup-v2)).
It checks the kernel release, user-namespace controls,
current-user subordinate UID/GID ranges, `newuidmap`/`newgidmap` and rootless
helper binaries, cgroup v2 CPU/memory/pids delegation, the current user's
systemd runtime sockets, and the expected package manager for the exact recipe.

The Debian 13 recipe uses Docker's signed Trixie APT repository; Ubuntu 24.04
uses Noble; Pop!_OS 24.04 uses Docker's Ubuntu Noble repository and checks the
Pop!_OS Ubuntu archive mirror or the official Ubuntu archive for helper
packages. Each path uses the bundled Docker key pinned to its approved
fingerprint, refuses conflicting source files, refreshes signed metadata,
checks candidate origins, and installs only Docker Engine, CLI, containerd,
rootless extras, and the distro's `uidmap` and `dbus-user-session` packages.
Fedora 45 uses the exact signed Docker stable DNF5 repository and checks Docker,
Fedora, and Updates package origins before installing Docker Engine, CLI,
containerd, rootless extras, `shadow-utils`, and `slirp4netns`.

A custom `XDG_CONFIG_HOME` is unsupported for automatic user-unit setup; that
task stays manual because the unit writer only manages
`~/.config/systemd/user`. APT-family Docker package scripts may install, enable,
or start the system rootful service; a separate consent is required. Fedora's
package operation does not activate the rootful service. The app never stops,
disables, or changes a rootful service. The subordinate-ID operation adds a
fixed range only when both current-user mappings are absent; partial or split
mappings remain manual. The service operation writes the fixed Kool.ad/e user
unit and starts/enables it only when inactive. Engine readiness separately
checks rootless identity, security options, resource limits, staging capacity,
and approved images.

Linux endpoint discovery derives `/run/user/<uid>/docker.sock` from the current
UID, independent of shell exports and desktop-launch environment forwarding.
It accepts `DOCKER_CONTEXT=rootless` only when safely owned local metadata names
that exact socket, rejects other non-empty contexts, and accepts `DOCKER_HOST`
only when it names the same socket. A private user-local record persists the
current UID, socket owner, and hashed endpoint and daemon fingerprints. It
stores no path, environment override, daemon ID, or credential, and invalidates
when the UID, socket owner, endpoint, or daemon changes. Rootful
`/var/run/docker.sock` is not selected as a fallback. See the [Docker
Engine prerequisite and manual setup/rollback
workflow](docker-engine-prerequisites.md#manual-setup-and-rollback).

Docker's [official rootless-mode prerequisites and setup
guide](https://docs.docker.com/engine/security/rootless/) and [systemd
user-service guidance](https://docs.docker.com/engine/security/rootless/tips/)
are reference material only. They do not establish that Kool.ad/e has tested or
approved a package recipe for a particular distribution. Rollback removes only
the current-user app unit and APT or DNF files when records prove this operation
created them. Package removal and user-data deletion are separate operator
decisions; retain Docker Engine packages if their removal could affect the
system rootful service. See the [Engine prerequisite and manual setup/rollback
workflow](docker-engine-prerequisites.md#manual-setup-and-rollback) for exact
paths and safety checks.

The Docker Desktop transport and bounded staging paths for macOS and Windows
are implemented, but native security and task-lifecycle validation is still
required before those hosts are enabled. The macOS endpoint probe now checks
Docker.app's signed bundle identity and version before discovering its local
socket. The catalog does not yet record the physical Mac architecture under
Rosetta or certify the publisher pin against current Intel and Apple silicon
releases. Windows edition, WSL2 kernel, reboot, and policy state also remain
unprobed. These facts and native host acceptance remain open under #348 and
#349. Synthetic platform fixtures validate classification; they do not
establish support on those hosts.
On Windows 11 x64, the read-only host probe reports edition/build/architecture, WSL and
Virtual Machine Platform feature states, WSL kernel version, pending restart
markers, the WSL policy setting, and whether virtualization is available to
Windows. It uses signed tools from the Windows system directory and does not
change host settings. Readiness results produce operator-guided setup tasks;
Kool.ad/e does not enable Windows features, change pipe ACLs or policy, restart
Windows, or change firmware settings. Docker Desktop trust and native task
execution remain unverified until Windows end-to-end validation passes.
Windows 10 and ARM64 hosts remain unsupported until their separate native
end-to-end acceptance evidence is completed.
macOS and Windows native execution remains blocked pending native Docker
Desktop staging, security, and runtime validation. Synthetic platform
fixtures validate classification; they do not establish support on those
hosts.

This readiness layer does not install packages, configure services, pull images
without explicit approval, or execute model-provided shell commands. A supported
remediation operation uses a typed action, required operator approval and
native privilege confirmation, and an independent readiness probe. If a coding
CLI is absent or unusable, deterministic setup instructions remain available
without agent assistance.

## Acceptance evidence and remaining work

The automated suite covers typed evidence and dependency consistency,
redaction, stale generations, one-provider task selection, exact image
prerequisites, task-store lifecycle, the UI result-acceptance boundary,
approval and journal behavior, package-source validation, apt update/candidate
failure handling, subordinate-ID classification, user-service failure and unit
creation/idempotence, and endpoint ownership and conflict checks. Scripted
service tests assert that only current-user `systemctl --user` operations are
issued; they do not prove coexistence with a live rootful service. These tests
do not install host packages or validate a live systemd user manager. The
Docker CI daemon is pre-provisioned, so rootless execution tests are not
bootstrap acceptance.

Human acceptance remains required on disposable Ubuntu 24.04, Debian 13,
Fedora 45, and Pop!_OS 24.04 amd64 VMs where runnable: package and
native-elevation flows, startup with an existing rootful Docker service left
untouched, desktop launch without shell exports, rootless Engine verification
and approved-image smoke, reboot/relogin, and repeated idempotent scans. No
recipe should be described as production-ready before that evidence is
recorded. Human validation is also required for actual provider conversation,
Docker MCP routing, novice first-run usability, visual review at
360/900/1280/1600 px, keyboard and screen-reader use, and reduced motion.
Release acceptance also requires Git/provider readiness; macOS Docker Desktop
on Intel and Apple Silicon; Windows WSL2 x64; signed native packaging; and the
full security/usability gate tracked by #342–#351. Mocks or Linux CI do not
certify these hosts or setup workflows.

Native package build artifacts are unsigned and CI-only. They do not change the
support matrix; see [native desktop packaging status](native-desktop-distribution.md).
