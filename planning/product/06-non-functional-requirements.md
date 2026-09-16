## 6. Non-Functional Requirements

- **NFR-1 Storage:** Shared planning truth MUST be git-backed. Private chat/checkpoints MAY survive restart, but are not product authority. Derived summaries or indexes MUST be rebuildable from project artifacts and repositories. Future collaboration transport remains transient (D-18, D-22).
- **NFR-2 Atomicity:** Individual artifact writes MUST be atomic; a multi-artifact planning turn MUST use a recoverable transaction so a crash cannot leave a partially accepted turn.
- **NFR-3 Consistency:** One active planning writer per application session; planning transaction locking spans processes sharing a git common directory. The channel-era writer model is ruled: true concurrent co-authoring, delivered through a convergence layer (merge/CRDT-class) composed with one-commit-per-turn apply in the channel's design phase (D-33); the per-session single pen stands until the channel ships.
- **NFR-4 Responsiveness:** UI progress and Cancel MUST remain available during long Pi turns. The default is twelve hours (D-24 supersedes D-08). No quantitative latency target is accepted (D-19).
- **NFR-5 Security:** Provider/MCP traffic remains Pi-owned. Git remote operations use the system git CLI; no shell interpolation is used for git arguments. Logical document IDs MUST map only to approved paths. The future WebSocket channel grounds peer trust in repository access, proved at connect time via the peer's git-derived identity; no Packet-minted accounts or secrets (D-32).
- **NFR-6 Extensibility:** `AiHarness` isolates Pi; alternate harnesses remain deferred (D-12, D-15, D-16). F-16 documents operator provisioning.
- **NFR-7 Deployment:** Rust edition 2024, offline-capable build, Linux x86_64 workstation launch from source (D-11; CLR-001 resolved). No packaged or cross-platform distribution commitment exists. The future channel may require a new runtime dependency (D-22).
- **NFR-8 Quality:** Changed behavior MUST have meaningful regression evidence and a passing full test suite. Tests MUST exercise validation/recovery and UI interaction where those are the behavior under change.
- **NFR-9 History:** Checkpoint subjects SHOULD be short imperative phrases; completed feature specifications and frozen task batches retain historical evidence in git.
- **NFR-10 Context scale:** Normal planning context MUST stay bounded as completed features, repositories, tasks, imports, and chat grow. Deterministic IDs and references precede optional semantic discovery (D-28).

The pending F-19/F-20 board work remains outside this feature's acceptance bar; D-07's private chat boundary, D-14 routing, and D-18/D-22 collaboration deferral remain intact. CLR-006, CLR-016 and CLR-018 are resolved historical items.
