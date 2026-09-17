## 11. Risks and Open Concerns

### Active concerns

- Channel design-phase residues: D-32 fixes the topology, reach, and trust ground, and D-33 fixes the writer model as true concurrent co-authoring; the concrete connect-time challenge/response and the convergence (merge/CRDT-class) layer's composition with one-commit-per-turn apply remain to be designed together (D-18, D-22; NFR-5).
- Auto integration may finish on the remote while a dirty/diverged local planning checkout cannot fast-forward. Packet preserves that checkout and reports the condition; reconciliation must be based on merged commit evidence. This is accepted operational debt, not an assertion that local files are synchronized.
- A multi-repository feature can publish individual repository tasks, but no cross-repository atomic release is promised. Coordinated release remains project-specific.
- Pre-existing clippy debt: 110 warnings across 37 files at 3ba5aa2 (Rust 1.98 clippy), no trace attributable to any recent feature diff. D-34's baseline-relative bar legally carries this debt forward; retiring it is a standalone lint-cleanup sweep owed as ordinary maintenance, deliberately excluded from feature-batch scope, and its absence is a known gap against the legacy globally-zero expectation.

### Recently resolved

- CLR-021: Closed by D-34 - NFR-8's warning bar reads as 'no new warnings versus the recorded baseline at the commit under verification', evaluated on the pinned Rust 1.98 clippy toolchain with mandatory re-baselining on toolchain upgrade; the initial baseline is 110 warnings at 3ba5aa2; pre-existing debt remains ordinary maintenance (active concern above). CHG-003's REQ-ALL-1/AC-6 were re-aligned to the ruling.
- CLR-016: Closed by D-33 - the channel's writer model is true concurrent co-authoring; a convergence layer (merge/CRDT-class) is priced at the front of the channel's design phase, composed with one-commit-per-turn apply. The per-session single pen (NFR-3) stands until the channel ships.
- CLR-015: Closed by D-32 - direct instance-to-instance WebSocket channel (instances listen and dial; no relay), endpoints manually entered from git-remote host information first, local-network reach for the first cut, and trust grounded in repository access proved at connect time via the peer's git-derived identity. The concrete handshake mechanic survives as a design-phase residue listed above.
- CLR-017: Closed by D-31 - the sharable surface is planning artifacts only (specification, open items, presence); interview and chat history stay operator-local, ratifying D-07. No privacy review is owed and $PACKET_HOME keeps its single store.

CLR-019's no-fetch observation is obsolete: D-30 requires an explicit target-branch fetch before implementation. CLR-001–CLR-014, CLR-016, CLR-018 and CLR-020 are historical resolutions recorded in the decisions log or git; they do not remain active questions. The old single-file drift (D-17) is superseded by D-28. The F-19/F-20 display work is deferred implementation, not a blocker for CHG-001.

No quantitative performance threshold is specified (D-19). New metrics or changes to the MVP acceptance bar require an explicit decision (D-20/D-21/D-24).
