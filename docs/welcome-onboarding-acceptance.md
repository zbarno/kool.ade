# Man.ager-guided Welcome

Implementation and acceptance ledger for #431. This document is a plan, not
validation evidence. No acceptance criterion is currently claimed complete.

## Journey

The initial card says “Tell Kool.ad/e what you want to make” and offers
“Get started”. A narrow progress rail tracks Meet your Man.ager, Prepare this
computer, Choose a project, and Review and open. Back preserves entered values.
At narrow widths the rail becomes a compact progress label above the card.
“Need help? Ask Man.ager” remains reachable without scrolling past the form.
Technical details and existing direct connection controls live under Advanced.

1. Meet your Man.ager explains the ongoing planning role, shows discovered
   providers and their verified capabilities, and opens existing install/login
   flows. Selection updates only the Manager and explicitly chosen bootstrap
   routes. Unverified capability uses deterministic help.
2. Prepare this computer summarizes authoritative setup tasks. Planning and
   coding readiness are separate. Repairs open existing consent and Task Details
   flows; the wizard never owns repair permissions or manufactures completion.
3. Choose a project offers recent workspaces, an existing folder, or a new idea.
   Recent cards use private successful-open timestamps and stable workspace IDs.
   Existing folders preserve Git state; non-Git folders require initialization
   consent. New projects require a name and intent, with separate code and
   planning locations shown before writes.
4. Review and open shows the exact locations and approved operations. A durable
   transaction resumes partial creation without deleting files or duplicating
   workspace identities. The initial idea enters the normal planning interview.

Returning users see recent workspaces first. Scoped readiness regressions offer
repair or safe planning instead of restarting onboarding.

## Integration contracts

- The app coordinator owns asynchronous operations and generation fences; egui
  produces intents only. Every result is matched to the session revision.
- A versioned private session records step, entry path, sanitized inputs,
  selected route, task references, transaction identity and timestamps. Persist
  atomically. Stored operation references never constitute renewed consent.
- `SetupTaskStore`, the remediation broker and capability probes remain the
  authoritative readiness and repair services.
- `WorkspaceManager` owns creation, join, open, repository registration and
  checkout mapping. Shared manifests never store personal recency or setup chat.
- Manager help reuses machine-local context-only conversations. Evidence and
  allowed actions come from app-owned typed state; host commands and credential
  collection remain forbidden.
- Existing migration, direct paths, remote joins, multiple code repositories,
  settings and coding routes remain accessible and require regression checks.

## Evidence required before completion

- Real egui click, keyboard, focus and resize tests plus wide/narrow screenshots.
- Real local Git integration: existing worktree/index/HEAD preservation, consent
  for initialization, separate repositories, stable linkage and intent handoff.
- Restart/failure cases: duplicate clicks, stale results, denied permissions,
  occupied paths, interrupted creation, failed commit/clone and expired consent.
- Negative tests: no arbitrary host execution, automatic elevation, remote push,
  secret ingestion, overwrite, destructive rollback or unsafe Docker fallback.
- Successful-open MRU ordering, stable migration order and reconnect identity.
- Real supported Linux first-run smoke and an observed novice usability session;
  document confusion and remediate actionable findings. Synthetic tests alone do
  not satisfy these requirements. macOS/Windows require their own host evidence.
- Independent agent validation, remediation of blocking findings, all four local
  Rust gates and required hosted checks on the final PR revision.

For each added integration test, document its production boundary, invariant,
why unit coverage is insufficient, external dependencies and setup cost.
