# Machine bootstrap and agent-guided setup

Tracking: [#337](https://github.com/zbarno/kool.ade/issues/337).

## Implementation status: slices A-C (incremental foundation)

Kool.ad/e starts bounded coding CLI discovery before Docker readiness, even
from the Welcome screen. Unknown authentication or model access fails closed;
the app does not silently select another provider. The typed readiness catalog
now produces independent findings for coding agents, Docker and approved
images.

Readiness findings are reconciled into a machine-local task collection under
operator state, outside project repositories. The versioned store migrates the
previous schema, including generic Docker/provider cards that must be rechecked
before they can be replaced by specific findings. It fences stale scans with a
durable generation,
uses atomic writes and an inter-process lock, and bounds task, attempt and event
history. A validated dependency DAG gives stable task order, detects cycles and
projects unresolved prerequisites as blockers. Fresh, domain-matched probe
evidence is required to verify a task; a regression reopens it. Restart
recovery interrupts in-flight work and removes active authorization bindings,
so an operation or approval is never replayed automatically.

The Welcome screen projects the task collection, and Kanban shows distinct
cards with setup task details.
Attempt and authorization records are typed audit metadata only: this store
does not execute repairs, grant host access or make a recorded receipt an
authority to mutate the machine. The separate remediation, assistant and
platform-onboarding issues remain incremental work. Do not describe these
slices as complete hands-free onboarding or certified cross-platform support.

## Safety contract

- A Docker or image finding is created only from its own typed probe evidence.
  An unavailable, stopped, rootful or untrusted daemon does not prove an image
  is missing.
- Machine setup records live under operator-local app state and are not added
  to `.koolade-packet`, project planning Git, task clones or shared team state.
- Task identity is stable across workspaces on one machine; a new scan cannot
  duplicate an existing prerequisite or overwrite a newer observation.
- A verified state requires fresh authoritative evidence for that task's
  readiness domain. A model response, unrelated failure or stale scan is not
  evidence of success.
- Persisted approval metadata does not execute an operation. Any future
  remediation must use a typed allowlist, genuine user consent, any required
  native privilege prompt, and a new independent readiness probe.
- The bootstrap assistant can explain choices and request typed actions. It
  cannot invent or execute host shell commands, elevate itself or override
  Docker rootless policy. Deterministic setup guidance remains available when
  no coding CLI is installed or authenticated.
- Runtime QA remains disabled until #171 passes.

## Remaining validation and work

Human validation is still required for the novice first-run flow and real-host
setup behavior. Linux rootless remediation and smoke validation, macOS Docker
Desktop (Intel and Apple Silicon), Windows WSL2 x64, signed native packaging,
and the complete security/usability release gate remain tracked by #342–#351.
Mocks or Linux CI do not certify those platforms.
