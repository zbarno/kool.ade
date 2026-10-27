# Coding tool discovery

Kool.ad/e asks the installed Codex app-server for its paginated, non-hidden model catalog and shows the reported slugs and default in Coding tools settings. OpenAI documents that `model/list` can return a bundled or cached catalog; it is not an account entitlement check. For current account-specific choices, OpenAI recommends querying `/v1/models` with that account's OAuth access token. Kool.ad/e does not read a Codex credential store or make that account-specific request, so a listed model can still be unavailable to the signed-in account. Codex task routes are currently blocked because Kool.ad/e cannot contain organization-managed hooks. For enabled providers, the application manages MCP tools and repository commands run through the application-owned Docker ExecutionSession.

Sources: [Codex app-server](https://developers.openai.com/siwc/token-sharing-open-source/codex-app-server), [Models and inference](https://developers.openai.com/siwc/token-sharing-open-source/models-and-inference).


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
