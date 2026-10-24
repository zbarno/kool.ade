# Machine bootstrap and agent-guided setup

Tracking: [#337](https://github.com/zbarno/kool.ade/issues/337).

## Implementation status: slice A (initial foundation)

Kool.ad/e now starts a bounded **coding CLI discovery before Docker readiness**
even on the Welcome screen. The same six provider probes as Coding Tools are
used with Docker execution marked unavailable. Only independently verified
authentication, fresh model evidence and safe context-only planning can qualify
an agent for bootstrap selection. Unknown auth/model access fails closed.
Kool.ad/e does not silently select an alternate provider.

At this stage, the existing board still projects **one active setup card**.
CLI setup problems are surfaced before Docker problems, and the first-run
screen links to Coding Tools. This slice does **not** yet enable a separate
agent-run setup conversation, install packages, pull images automatically, or
generate multiple persistent setup tasks. Do not describe it as complete
hands-free onboarding. The full work plan is in #337.

## Contract for subsequent implementation slices

- Typed and independent per-prerequisite results; never label an image missing
  merely because Docker is rootful, stopped or inaccessible.
- Stable host-local task identity, revision-fenced state and dependencies; no
  machine setup cards committed to a shared planning Git repository.
- A supported predetermined remediation operation requires the user's
  permission, additional native privilege confirmation when necessary, and a
  **new independent readiness probe** after execution.
- The bootstrap agent can explain, translate and request typed actions. It
  cannot invent or execute native shell commands, grant itself host privileges
  or override Docker rootless policy.
- If no CLI is installed or authenticated, setup still uses deterministic
  explanations and actions. Runtime QA remains disabled until #171 passes.

## Acceptance sequence

On a disposable Linux machine, verify (1) CLI discovery while Docker is
rootful or absent, (2) an installed but unauthenticated/unknown provider is
not considered usable, (3) an explicitly selected verified agent remains
eligible for context-only assistance, and (4) Docker readiness is reported
independently. Later slices add rootless setup, image preparation, task
persistence, guided remediation, and full end-to-end dogfood evidence.
