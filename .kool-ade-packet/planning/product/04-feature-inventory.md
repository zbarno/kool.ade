## 4. Feature Inventory

Statuses describe current repository behavior; pending items are not treated as shipped design.

| ID | Capability | Status |
| --- | --- | --- |
| F-1 | Connect and survey a git repository | Implemented |
| F-2 | Interview chat, one turn at a time, with Cancel | Implemented |
| F-3 | White-paper Markdown specification view and document switcher | Implemented |
| F-4 | Open-item and task Kanban with detail/activity views | Implemented |
| F-5 | Snapshot, context, Pi, validation, transactional apply, checkpoint pipeline | Implemented |
| F-6 | Strict operation-specific wire responses with logical document updates | Implemented; D-05 superseded by D-29 |
| F-7 | Stable open-item lifecycle and separate authority/priority | Implemented |
| F-8 | Seat-scoped, ownership-aware routing | Implemented; D-14 |
| F-9 | Scoped git checkpoints | Implemented |
| F-10 | In-repository document import | Partial; import dialog dogfood remains |
| F-11 | MCP configuration stored locally; bounded server-name summary supplied to planning context | Implemented; configuration commands and credentials are withheld |
| F-12 | Live worker and planning activity | Implemented |
| F-13 | Welcome and first-run guidance | Implemented |
| F-14 | Private operator chat/checkpoint persistence | Implemented |
| F-15 | Configurable turn budget | Implemented; 12-hour default D-24 supersedes D-08 |
| F-16 | In-app Pi setup guide | Implemented; D-12, D-13, D-15 |
| F-17 | Stakeholder/ownership settings | Implemented; D-14 |
| F-18 | In-app MCP editor | Implemented; D-16 |
| F-19 | Red line activity graph on task cards | Implemented; D-26; CHG-002 |
| F-20 | Board card colors by class | Implemented; D-27, CLR-020; CHG-002 |
| F-21 | Modular current product and per-feature change specifications | Implemented; CHG-001 |
| F-22 | Bounded, deterministic, role-specific context compilation | Implemented; CHG-001 |
| F-23 | Planning-root repository manifest and single-target dependent tasks | Implemented; CHG-001 |
| F-24 | Agent/Review/Human uncertainty and board approval | Implemented; CHG-001 |
| F-25 | Approved-feature implementation gate and merged-code reconciliation | Implemented; CHG-001 |

The WebSocket channel remains deferred (D-18, D-22; CLR-014 resolved) and has no F-number yet. NFR-2, NFR-5, NFR-6, and NFR-8 constrain these capabilities; FR-13 remains the identity contract.
