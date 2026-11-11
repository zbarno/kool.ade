# Coding tool discovery

Kool.ad/e asks the installed Codex app-server for its paginated, non-hidden model catalog and shows the reported slugs and default in Coding tools settings. OpenAI documents that `model/list` can return a bundled or cached catalog; it is not an account entitlement check, so a listed model can still be unavailable to the signed-in account. Login banners and catalog listings therefore never mark Codex ready; account access is established only by the bounded live verification described below. For enabled providers, the application manages MCP tools and repository commands run through the application-owned Docker ExecutionSession.

Sources: [Codex app-server](https://developers.openai.com/siwc/token-sharing-open-source/codex-app-server), [Models and inference](https://developers.openai.com/siwc/token-sharing-open-source/models-and-inference).

## Account access verification (Codex)

Discovery establishes Codex account access with one bounded live call:
a tiny `codex exec` round trip in a read-only, ephemeral sandbox with a 60-
second timeout and output caps. A clean exit with a reply proves the signed-in
account can actually run a model; anything else stays “authentication
unverified”, and the diagnostic is redacted so credential-shaped text never
reaches settings, logs, or UI. Routine discovery spends at most one such call;
opportunistic checks that only need to refuse execution rely on local evidence
and a stored receipt instead of spending a model call.

A successful verification is persisted as a per-provider receipt for 24 hours.
The execution gate trusts a fresh receipt without calling a model again, so
relaunches do not burn repeated calls; an expired receipt simply re-verifies at
the next discovery. Changing an executable path or otherwise intervening in a
provider invalidates its receipt.

Background discovery sweeps start once at app start, then only on an explicit
request (the dialog’s per-provider “Check” button, “Rediscover tools”, or an
unroutable-provider notice). Nothing re-scans on a timer; discovery freshness
decides presentation, not scheduling.

Codex task routes remain blocked while organization-managed hooks cannot be
contained inside the app's execution boundary; use another provider for
Kool.ad/e tasks meanwhile.


## Role routes

Coding tools Settings can select a provider and model for Manager, planning,
specification, feature planning, task generation, investigation, independent
plan review, implementation, code QA, documentation, reconciliation, decision
explanation, and repair. New task conversations choose their role route from
the work kind, and saved task overrides apply only to that category. Manager
can use a global or project route, while a task cannot change its application-
owned authority. Project Settings stores route overrides with the project. The
precedence is task override, project role route, global role route, selected
default, then the legacy Codex setting. Each route keeps the role's existing
`ExecutionMode`; provider choice does not change Docker tool authority. An
unavailable saved route reports its blocker instead of switching providers or
models. Runtime QA review routing remains separate from its approved driver
and runtime policy gate.

## Provider setup task priority

Each provider detail has a separate “Prioritize this provider for setup tasks”
choice. It selects the provider Kool.ad/e can use for guided setup; the selected
Default tool keeps its own readiness tasks when it is a different provider.
This choice does not route conversations. Conversation routing continues to
follow the Default tool and work-type routes above, with Manager remaining
authoritative. If no provider is selected for guided setup, the selected Default
tool is used as the setup candidate. Discovery does not pick Pi or another fallback
automatically. See [machine bootstrap guidance](machine-bootstrap.md) for
provider-specific evidence, the reviewed Claude Code installation recipe, and
official installation links for other host/provider combinations.

Claude Code remains discoverable for version and sign-in checks when no
detectable local managed host policy blocks those probes, but Kool.ad/e does
not launch Claude tasks until it can sandbox organization-managed hooks.
Restricted mode and inline settings cannot guarantee those hooks stay inside
the app's execution boundary; use another provider for Kool.ad/e tasks meanwhile.
See Anthropic's [managed hooks documentation](https://code.claude.com/docs/en/hooks).
