# Coding tool discovery

Kool.ad/e asks the installed Codex app-server for its paginated, non-hidden model catalog and shows the reported slugs and default in Coding tools settings. OpenAI documents that `model/list` can return a bundled or cached catalog; it is not an account entitlement check. For current account-specific choices, OpenAI recommends querying `/v1/models` with that account's OAuth access token. Kool.ad/e does not read a Codex credential store or make that account-specific request, so a listed model can still be unavailable to the signed-in account. Codex is routed through Kool.ad/e's application-managed MCP tools; its native repository tools are disabled, and repository commands run through the application-owned Docker ExecutionSession.

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
