# F11 - User-local implementation telemetry and cost reporting — task stories
<!-- koolade-artifact-id:v1 {"uid":"ddbb6769-ec92-42c8-9651-7a91475c7109","displayId":"BATCH-DDBB6769","title":"F11 - Durable operator-local implementation telemetry (zbarno/kool.ade issue #23)","parentUid":"8b0cc126-4ef4-4a5b-9a2d-7452e96c2cae"} -->

**Planning snapshot:** [PR #54](https://github.com/zbarno/kool.ade/pull/54) closed Issue #23 and implemented telemetry. Three of nine planned stories are preserved here; this is a partial historical breakdown, not outstanding implementation work.

**Known gap:** This plan's R6 specifies a versioned price-table fallback. PR #54 leaves cost as “Not reported” when the provider supplies no cost, so that fallback was not implemented.

Only the three listed stories are part of this snapshot; it does not claim to cover the full issue.

[Approved specification](specification.md)

1. [Define versioned JSONL invocation record and telemetry storage](F11-TASK-define-versioned-jsonl-invocation-record-and-telemetry-storage.md)
2. [Normalize Pi harness usage into incremental records](F11-TASK-normalize-pi-harness-usage-into-incremental-records.md)
3. [Guarantee exactly-once and reconciled token counting](F11-TASK-guarantee-exactly-once-and-reconciled-token-counting.md)
