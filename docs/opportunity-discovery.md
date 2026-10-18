# Opportunity discovery and user authority

Manager opportunity proposals are advisory. The Manager emits bounded proposals alongside its ordinary update; they are not queue work, planning goals, tasks, approvals, or permission to edit a repository. Manager v5 distinguishes evidence, inference, confidence, workaround, user value, impact, risk, and deduplication basis, and requires a citation to application-supplied generic-capability evidence. Planner v3's typed request can carry opportunity origin, current support evidence, workaround, user value, confidentiality class, and concrete target repository. v2 and v3 Manager prompt identities remain readable for persisted runs.

## Target and evidence checks

Every proposal names a configured repository ID and the exact portable repository identity associated with it. The application validation context identifies the active checkout and the Kool.ad/e product checkout. `ActiveUserProject`, `KooladeProduct`, and `OtherLinkedRepository` must match those identities. A proposal targeting another repository must cite evidence explicitly marked public-safe; private workspace evidence cannot cross that boundary. Proposals for the active user project may use that workspace's evidence. Evidence IDs must be supplied, unique, and bounded. Confidence is a closed enum, and a speculative proposal cannot claim high confidence.

A generic recipe, tool, or capability that already solves the immediate task must be considered before claiming a missing capability. A proposal should identify the current support state, workaround, duplicate check, expected benefit, tradeoffs, and a narrowly scoped approach. Model-provided severity, confidence, or proposal text never grants authority.

## User decision

The typed decision receipt binds an authenticated user ID to one opportunity ID and exact SHA-256 proposal revision. It also requires a unique receipt ID and RFC 3339 timestamp. The model output schema rejects decision and approval fields; the decision validator accepts the authenticated user identity separately from the untrusted proposal output. `Pursue` means permission to begin normal goal clarification and planning only. Plan confirmation, independent review, task execution, dependency access, and PR authorization remain separate gates.

## Decision matrix

| Observation | Propose when | Continue current work when |
| --- | --- | --- |
| A generic recipe/tool/capability exists | A native integration has evidenced reuse or user-experience benefit beyond the working recipe | The existing path satisfies the goal and no concrete additional benefit is shown |
| Repeated failure, QA defect, review finding, or spec drift | A typed finding is tied to current workspace evidence and a concrete repository target | The signal is transient, already fixed, unsupported by evidence, or duplicates an existing/rejected proposal |
| Roadmap or user-goal opportunity | The user value and likely outcome are stated, with uncertainty and tradeoffs | The idea is speculative, out of scope, or would interrupt unrelated approved work |
| Security concern | Surface urgently with its evidence and target | Never let model severity bypass the normal user-authorized repair and publishing gates |

## Current implementation and remaining integration

The prompt catalog, strict proposal validation, repository identity matching, cross-repository evidence filter, and decision receipt validator are implemented. Proposal persistence recomputes the workspace/target/problem dedup key and commits a per-UTC-day quota reservation atomically with the advisory record. The workspace-local exploration settings default to disabled and require a revision-checked explicit opt-in; scheduled exploration proposals are rejected until enabled. The daily limit is configurable from 1 to 10 proposals and applies to all origins.

The Manager callsite receives a workspace-private capability inventory derived from the validated project runtime manifest. If the inventory is missing or invalid, it is explicitly reported as unknown rather than treated as proof of missing support. It also receives recent typed lifecycle receipts and typed QA results loaded through `RuntimeQaStore::load_run`, typed implementation queue failures, and sanitized typed dependency broker outcomes. Event-driven origins require matching application evidence IDs; arbitrary model strings do not satisfy that requirement. When the Kool.ad/e repository is linked, the prompt also receives this committed public product contract as target-bound evidence; user workspace evidence cannot be used for a Kool.ad/e or other-repository proposal. Dismissal is persisted as `Declined`; `Not now` is persisted as `Deferred` with a seven-day expiry, and the board offers 1, 7, or 30 day snooze actions. Repeated evidence stays suppressed. A declined proposal can be surfaced again only when new evidence arrives; a deferred proposal remains suppressed through its snooze and can be surfaced again only after expiry with new evidence. Each surfaced follow-up is a new advisory record; previous decisions remain in history. A direct user feature request continues through the normal planning flow and cannot be inferred from a model-provided opportunity origin.

The Manager schema requires the exact generic-capability inventory evidence reference in both the proposal's evidence list and support-check text. The inventory itself is application-generated from validated persisted runtime data, so proposal wording cannot invent its contents.

The durable proposal/decision flow, deduplication, quota reservation, persisted opt-in policy, settings UI, snooze controls, authenticated receipt creation, and lifecycle planning adoption are integrated. Scheduled exploration is disabled by default. When explicitly enabled, the app queues at most one exploration per seven days while the workspace is idle; the ordinary Manager path applies the configured daily proposal quota, and no implementation or planning action follows without a separate user decision. This scheduler runs through the application tick, not as a background service.
