# Task conversation acceptance audit

Authority: user attachment `1196af88-c30f-4390-b755-754dbd16561d/pasted-text-1.txt`.

## Requirement coverage

| Requirement | Implementation and evidence |
| --- | --- |
| Main Chat remains the project-level interface | Existing main turn path remains intact. Focused turns reject interview/task-generation fields. The UI fixture verifies Main Chat remains unchanged after two task replies. |
| One persistent isolated conversation per item | TaskChats keys histories and drafts by stable item identity or task-story path. A 510-message history and a second stream survive reload. Generated ownership identities survive permanent numbering and artifact reload; archived identities are not reused. |
| Lightweight inline replies | Every question and task-story card uses the compact renderer: latest message, single-line input, Send reply. The egui interaction fixture types and submits on the board. |
| Expand into the same conversation | Card titles open the existing details modal. Both surfaces use the same draft/history. The interaction test carries unsent drafts in both directions, submits from both surfaces, and reloads all four messages. |
| Description, metadata, history, larger input, artifacts | Existing question metadata/evidence/recommendation and task-story description/properties/activity remain in the modal. Expanded conversation adds complete scrollable history and a multiline input. Existing board/modal and viewport tests remain in the regression suite. |
| Concise task context from durable state | The focused prompt reads the selected item/story, implementation status, explicitly referenced items/tasks, historical feature documents, and referenced specification sections. Tests cover headings/bullets/tables, exact identifier matching, legacy specifications, and explicit dependencies while excluding unrelated content. |
| No implicit history inheritance | Focused requests take only the selected stream. Captured UI-fixture prompts exclude distinctive Main Chat and other-task sentinels, and include the selected task's prior exchange. |
| Important outcomes update shared state | Existing validation and atomic apply/checkpoint pipeline handles replies. Tests prove evidence updates, specification changes, resolution, archived outcomes, and reopening. Main planning context reads durable outcomes instead of task transcripts. |
| Focused communication, not autonomous planning sessions | System instructions and envelope guards prohibit advancing the project interview, generating batches, or redirecting the next question to another item. Implementation controls remain separate. |
| Small routine interaction; deeper discussion in modal | Cards show only the latest message and compact controls. Expanded history scrolls. Send disables during planning/reconciliation while drafts remain editable. Stop reply targets only the selected stream. |

## Recovery and regression evidence

- `inline_and_modal_replies_share_history_and_keep_other_chats_out_of_prompts`: egui pointer/text events, controlled provider completion, state application, draft/modal continuity, archive access, restart persistence, and prompt isolation.
- `rejected_failed_and_cancelled_replies_stay_in_task_and_preserve_project_state`: rejected workflow changes, provider failure, and cancellation preserve shared state and persist task-local notices without changing Main Chat. Cancellation targets only the active key.
- `synthetic_ownership_conversation_keeps_identity_after_numbering_and_reload`: a generated ownership card accepts a reply and retains its conversation after receiving a CLR ID.
- Persistence tests verify stale-window merging under a file lock, exactly-once retry of retained responses, corruption protection, separate histories, and no truncation at the main-chat retention limit.
- Board approval uses the same archive/apply path; its fixture verifies the resolved conversation identity after reload.
- Focused turns check shared-state freshness before apply. Existing routing, atomic artifact, implementation/PR, layout, generation, and integration suites remain part of the regression run.

## Operational behavior

Histories remain per-operator runtime state outside Git. Shared specifications, item evidence, and completed-item outcomes remain Git-backed state. Disk-save errors are visible; unsaved agent responses stay in memory for explicit retry, so the window should remain open until saving succeeds. Agent validation uses deterministic provider fixtures; this audit does not claim live-model answer quality.

## Final checks

- `cargo test --offline --quiet`: 249 library tests plus both integration tests passed. The task rejection fixture was then strengthened to emit a structurally valid interview update; its focused rerun also passed.
- `cargo build --offline`: passed for the production application.
- `git diff --check`: passed.
- One earlier final-suite attempt encountered contention while acquiring the test's deliberate failure-injection lock. The fixture now establishes that lock with a blocking acquisition before exercising the production nonblocking save path; the subsequent full suite passed.

All requirements from the attached task are covered above. Changes are left in the working tree; no commit or publication was requested.
