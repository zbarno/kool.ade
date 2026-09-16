## 11. Risks and Open Concerns

### Active concerns

- CLR-015: Post-MVP collaboration topology, discovery, authentication, and trust remain undecided (D-18, D-22; NFR-5).
- CLR-016: A future multi-client writer/merge model remains undecided (NFR-3).
- Auto integration may finish on the remote while a dirty/diverged local planning checkout cannot fast-forward. Packet preserves that checkout and reports the condition; reconciliation must be based on merged commit evidence. This is accepted operational debt, not an assertion that local files are synchronized.
- A multi-repository feature can publish individual repository tasks, but no cross-repository atomic release is promised. Coordinated release remains project-specific.

### Recently resolved

- CLR-017: Closed by D-31 - the sharable surface is planning artifacts only (specification, open items, presence); interview and chat history stay operator-local, ratifying D-07. No privacy review is owed and $PACKET_HOME keeps its single store.

CLR-019's no-fetch observation is obsolete: D-30 requires an explicit target-branch fetch before implementation. CLR-001–CLR-014, CLR-018 and CLR-020 are historical resolutions recorded in the decisions log or git; they do not remain active questions. The old single-file drift (D-17) is superseded by D-28. The F-19/F-20 display work is deferred implementation, not a blocker for CHG-001.

No quantitative performance threshold is specified (D-19). New metrics or changes to the MVP acceptance bar require an explicit decision (D-20/D-21/D-24).