## 10. Decisions Log

| ID | Decision | Basis | Status |
| --- | --- | --- | --- |
| D-01 | Root `SPECIFICATION.md` governs the initial MVP. | Initial contract | Superseded by D-17 |
| D-02 | Use a Rust eframe/egui desktop UI. | Repository baseline | Observed / needs ratification |
| D-03 | Use system git and `Packet Planner` checkpoint authorship. | Repository baseline | Observed / needs ratification |
| D-04 | All LLM activity runs through external Pi, not embedded inference. | Existing contract | Confirmed |
| D-05 | Planner responses use schema-v1 `updated_specification`. | Original contract | Superseded by D-29 for modular projects |
| D-06 | App-minted `CLR-` IDs are stable and never reclaimed. | Repository baseline | Observed / needs ratification |
| D-07 | Chat history stays operator-local. | Existing behavior | Ratified by D-31; CLR-017 resolved |
| D-08 | Default turn budget is two hours. | Initial contract | Superseded by D-24 |
| D-09 | Stream polling distinguishes timeout from process termination. | Regression result | Observed / needs ratification |
| D-10 | Rust edition 2024 and thin-LTO release profile. | Manifest | Observed / needs ratification |
| D-11 | MVP launch is from source on the operator's Linux x86_64 workstation. | Operator decision | Confirmed; CLR-001 resolved |
| D-12 | Operator provisions Pi; other harnesses and in-app installers are deferred. | Operator decision | Confirmed |
| D-13 | Discover Pi via override, PATH, common locations; display any installed version without pinning. | Operator decision and code | Confirmed; CLR-002 resolved |
| D-14 | Identity is git-first; explicit person ownership is sole, groups share, unowned lanes seat-inherit. | Operator decision | Confirmed; CLR-003 resolved |
| D-15 | The Pi setup guide belongs inside settings, not a per-repository document. | Operator decision | Confirmed |
| D-16 | Edit MCP JSON in-app; warn on malformed JSON but allow save; blank removes it. | Operator decision | Confirmed; CLR-004 resolved |
| D-17 | `planning/specification.md` is the single standing product authority; root contract is provisional. | Operator decision | Superseded by D-28; CLR-005 resolved |
| D-18 | A future WebSocket channel may provide live peer collaboration over git-backed truth. | Operator decision | Confirmed direction; CLR-006 resolved |
| D-19 | No quantitative performance/usability target binds the MVP exit bar. | Operator decision | Confirmed |
| D-20 | Use a scripted fixture-repo demonstration plus regression suite for MVP acceptance. | Operator-delegated proposal | Confirmed by D-21; CLR-007 resolved |
| D-21 | Ratify D-20 without amendment; change the MVP bar only by fresh ruling. | Operator decision | Confirmed |
| D-22 | Defer the D-18 channel until post-MVP; preserve git as durable truth. | Operator decision | Confirmed; CLR-014 resolved; the earlier no-network observation is narrowed by D-30 git transport |
| D-23 | Seat this project under its connected git identity, Zachary Barno. | Operator decision | Confirmed |
| D-24 | Set default turn timeout to twelve hours; invalid override falls back. | Operator decision | Confirmed; supersedes D-08; CLR-018 resolved |
| D-25 | Zachary Barno is sole owner of all six non-General lanes in this project. | Operator settings action | Confirmed; CLR-008–CLR-013 resolved |
| D-26 | Show the observed red-line activity graph on task cards while preserving the activity metric. | Operator directive | Confirmed and implemented (F-19) |
| D-27 | Color board cards by work class, with a type-based palette. | Operator directive | Confirmed and implemented (F-20, CLR-020 resolved) |
| D-28 | Maintain one logical current product specification and concise feature deltas; preserve completed deltas and history in Git, while the board projects actionable work and context is selected on demand. | Operator CHG-001 proposal | Confirmed; supersedes D-17's physical single-file ruling. The original fixed module count is superseded by D-36. |
| D-29 | Modular planning uses strict operation-specific response schemas, a normalized internal projection, schema-v2 allowlisted `document_updates`, recoverable multi-artifact apply, authority-aware items, explicit new-feature approval, scoped single-repository task contracts, and merged-code reconciliation. | Operator CHG-001 proposal and architecture-remediation plan | Confirmed; supersedes D-05 for modular projects |
| D-30 | Fetch the latest target branch before implementation. | Operator directives and implementation evidence | Confirmed; resolves CLR-019's stale-base question. The combined Auto mode behavior is superseded by D-37. |
| D-31 | The sharable peer surface is planning artifacts only - specification, open items, presence; interview and chat history stay operator-local, ratifying D-07. | Operator decision (chat: "not sharing chats, just artifacts") | Confirmed; CLR-017 resolved |
| D-32 | Post-MVP channel shape: direct instance-to-instance WebSockets (instances listen and dial peers; no shared relay), endpoints learned first by manual entry from git-remote host information, local-network reach for the first cut; peers qualify by repository access, proved at connect time via their git-derived identity, with no Packet-minted accounts or secrets. | Operator ruling (CLR-015 chat: "can it just rely on the fact that they have access to the repo?", followed by "sounds good") | Confirmed; CLR-015 resolved |
| D-33 | Post-MVP channel writer model: true concurrent co-authoring; the channel's design phase prices and composes a convergence layer (merge/CRDT-class) with the one-commit-per-turn apply. Per-session single-pen operation stands until the channel ships. | Operator ruling (CLR-016 chat: "2", later "i want option 2") | Confirmed; CLR-016 resolved |
| D-34 | NFR-8's warning bar reads as 'no NEW clippy warnings versus the recorded baseline at the commit under verification', evaluated on the pinned clippy toolchain (initial pin: Rust 1.98, the version its first baseline was measured on); a toolchain upgrade mandates re-baselining, and the pre-existing debt (110 warnings across 37 files at 3ba5aa2) retires via a standalone lint-cleanup sweep scheduled as ordinary maintenance, never folded into feature-batch diff bounds. | Operator ruling (CLR-021 chat: "yes") | Confirmed; CLR-021 resolved |
| D-35 | Publishing grain alternative to Auto mode: per-feature PRs — the verified tasks of a feature integrate onto one integration branch and a single GitHub PR is published when the feature completes, with the card flipping Done and reconciliation driven by that PR's merge. Applies to batches launched after activation; in-flight batches keep their frozen contract, and the existing per-task PR mode remains untouched. | Operator ruling (main chat: "Option 2 — Per-feature PRs: build integration of all a feature's tasks onto one branch, one PR per feature at completion, reconcile on its merge") | Confirmed direction; implementation pending (future CHG; not current behavior) |
| D-36 | A product specification requires six concepts—Overview, Users and Outcomes, Current Capabilities, Architecture and Constraints, Decisions, Quality and Acceptance—and may add concise modules when the project's complexity needs them. Legacy thirteen-section specifications migrate losslessly into those concepts and retained optional modules. | Approved architecture-remediation plan, P10 | Implemented by the product manifest, migration, and planner policy; supersedes D-28's original fixed thirteen-module structure |
| CLR-029 | Store operator-chosen repository display names in the shared, git-checked-in repository manifest; keep machine-specific checkout paths private. | Operator choice; portable labels are shared while routing identity remains stable | Resolved and implemented |
| CLR-030 | Allow duplicate free-form display names, trim whitespace, cap at 40 characters, reject control/invisible formatting characters, and append the stable repository ID to missing or colliding labels. | Operator choice; preserves flexible naming while keeping every listing distinguishable | Resolved and implemented |
| D-37 | Automatic planning, building, and publication are independent project settings. Auto Plan may investigate Agent-owned items; Auto Build continues only explicitly approved work; Auto Publish controls remote integration and defaults off. Verified work remains local until the operator shares it or enables publication. | Approved architecture-remediation plan, P14 | Implemented; supersedes D-30's former combined Auto mode defaults while retaining its latest-target-branch fetch requirement |

Rows marked observed describe current implementation, not retroactively accepted product intent. Git history retains the detailed reasoning behind superseded rulings. CLR-017 is closed by D-31; CLR-015 is closed by D-32; CLR-016 is closed by D-33; CLR-021 is closed by D-34.
