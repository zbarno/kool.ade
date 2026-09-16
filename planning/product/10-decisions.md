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
| D-26 | Move a red line activity graph to task cards, preserving the activity metric. | Operator directive | Confirmed intent; implementation pending (F-19) |
| D-27 | Color board cards by work class, with type-based palette. | Operator directive | Confirmed intent; implementation pending (F-20, CLR-020 resolved) |
| D-28 | One logical current product specification lives in thirteen replaceable modules; each material change has a concise feature delta. Git keeps completed deltas and history, while the board projects actionable work and context is selected on demand. | Operator CHG-001 proposal | Confirmed; supersedes D-17's physical single-file ruling |
| D-29 | Modular planning uses schema-v2 allowlisted `document_updates`, recoverable multi-artifact apply, authority-aware items, explicit new-feature approval, scoped single-repository task contracts, and merged-code reconciliation. | Operator CHG-001 proposal | Confirmed; supersedes D-05 for modular projects |
| D-30 | Fetch the latest target branch before implementation; Auto mode defaults on, publishes a verified integration to the target branch, and advances the approved queue. | Operator directives and implementation evidence | Confirmed; resolves CLR-019's stale-base question |
| D-31 | The sharable peer surface is planning artifacts only - specification, open items, presence; interview and chat history stay operator-local, ratifying D-07. | Operator decision (chat: "not sharing chats, just artifacts") | Confirmed; CLR-017 resolved |

Rows marked observed describe current implementation, not retroactively accepted product intent. Git history retains the detailed reasoning behind superseded rulings. CLR-017 is closed by D-31; open post-MVP design items CLR-015 and CLR-016 remain in the board.