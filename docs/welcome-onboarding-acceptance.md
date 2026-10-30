# Man.ager-guided Welcome

Implementation and acceptance ledger for #431. Automated checks and independent
review are complete for the current follow-up; manual acceptance remains pending.

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
3. Choose a project leads with “Open an existing code project” and explains that
   the workspace list below contains planning repositories. Selecting a code
   checkout detects a valid sibling `<code>.packet` repository, or lets the
   operator browse directly to an existing planning folder or choose a listed
   workspace. The code folder is inspected automatically; the selected planning
   repository is joined by its manifest identity. A prominent “Connect and open
   project” button stays pinned at the bottom while the locations above scroll.
   It connects the selected folders and opens the workspace.
   Existing folders preserve Git
   state; non-Git folders require initialization consent. New projects require
   a name and intent, with separate code and planning locations shown before
   writes.
4. Review and open shows the exact locations and approved operations. A durable
   transaction resumes partial creation without deleting files or duplicating
   workspace identities. The initial idea enters the normal planning interview.

Returning users see recent workspaces first. Scoped readiness regressions offer
repair or safe planning instead of restarting onboarding.

## Integration contracts

- The app coordinator owns asynchronous operations and generation fences; egui
  produces intents only. Help responses match the session revision and route;
  folder inspections match the repository ID and exact folder; launcher results
  match the lookup generation; workspace loading matches the saved workspace ID.
  Recovery snapshots belong to the exact reviewed request.
- A versioned private session records step, entry path, user-entered project inputs,
  selected route, task references, transaction identity and timestamps. Persist
  atomically. Project inputs remain private until the approved planning request
  is saved; users must not enter secrets. Stored operation references never
  constitute renewed consent.
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
- Real supported Linux first-run smoke and the operator's manual usability session;
  document confusion and remediate actionable findings. Synthetic tests alone do
  not satisfy these requirements. macOS/Windows require their own host evidence.
- Independent agent validation, remediation of blocking findings, all four local
  Rust gates and required hosted checks on the final PR revision.

For each added integration test, document its production boundary, invariant,
why unit coverage is insufficient, external dependencies and setup cost.

## Current review findings

The follow-up implementation adds a pre-Docker provider choice without marking
Manager execution ready, step-specific machine-local help, responsive progress,
collision-aware location suggestions, and identity-fenced folder inspection.
The previous follow-up passed formatting, all-target nextest (3,662 passed; 17 skipped),
doc tests and Clippy with warnings denied after integration with main 52504d90.
Independent source review found no remaining blocking regression.

The existing-project flow was clarified after manual operator feedback: the
workspace list is labeled as planning repositories, opening code is the primary
action, and a valid sibling `.packet` repository is detected or can be selected
directly. The selected checkout is validated before initialization and joined
by its own manifest ID. Current source gates are rerunning for this update.

Reconnect validates a proposed association fully before publishing it. On Unix
filesystems it requires the original planning and Git directory device/inode
identities, workspace UUID, private relocation marker and last-opened ancestry.
It rechecks folder identity before publication. This supports same-filesystem
moves of the original checkout; clones, copied backups, cross-filesystem moves,
and legacy records without identity evidence are rejected. Windows relocation
needs a separate supported identity implementation; restore the original path
there. This does not prove that an external actor never rewrote history inside
the original Git directory. Recovery/import of replacement backups remains a
separate, unimplemented flow and is not claimed as accepted here.

The current GPU gallery captured 12 wide/narrow production-painter frames.
[Selected screenshots](images/welcome-onboarding/README.md) passed independent
visual review. Real egui tests cover clicks, Tab, Shift+Tab, Enter, Escape and
unapplied folder edits. Native focus/resize and manual usability remain separate
acceptance checks.

The current follow-up adds background workspace loading, launch-history events
that survive shutdown, separate readiness summaries, larger controls and a narrow
help sheet. New-project folder names can be edited before approval; changing the
location retains the original request and allocates new transaction identities.
Read-only recovery snapshots list observed folders, history entries, saved
connections and initial ideas. Existence alone never authorizes a retry.

The first-turn handoff remains available after provider failure until explicitly
dismissed. These follow-up changes have passed the local gates and synthetic visual review.
Hosted checks and the operator walkthrough remain before final acceptance.

On 2026-10-30, the operator explicitly waived the observed novice-session
requirement for this PR and will manually test on Pop!_OS/COSMIC. The manual
walkthrough remains pending. No novice validation is claimed.
