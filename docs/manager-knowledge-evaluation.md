# Manager knowledge integration and evaluation

## Source audit

Audited `main` at `160d17a4197f973fb3ea547b8343e204781ee103` on 2026-11-09.
This is a source audit of that revision, not a claim about a later release.

| Evidence | Current owner and data path | Authority and invalidation |
| --- | --- | --- |
| Public product support | Build-owned capability catalog (`src/app/manager/catalog/opportunities/evidence/`) | Describes product support/readiness; does not grant permission or prove target-project implementation. |
| Desired product behavior | Published spec candidates and reads (`src/app/manager/catalog/context/published.rs`) | Read from the primary code repository's published Git ref/SHA. Local validated decisions remain workspace evidence. A spec is intent, not implementation. |
| Local work state | Complete task index (`src/app/manager/prompt/index/`) and private planning/task state | Workspace-local. It may describe status or a human receipt; ordinary task text and a proposed plan do not authorize an action. |
| Repository architecture | Revision-keyed profile (`src/core/repository_knowledge/`) collected for Manager context by `src/core/context_retrieval/repository_knowledge.rs` | Derived, bounded, installation-local evidence. The profile records repository, ref, commit, dirty state, omissions and claim references. A changed commit/ref invalidates the key; dirty files are not presented as merged behavior. |
| Targeted source answers | App-authorized investigation (`src/app/manager/investigation.rs`) through `src/core/investigation_broker/` | Read-only, profile-indexed paths at pinned revisions with request/file/byte/time limits, cancellation, idempotency and hash-bearing receipts. A stale revision is not accepted. |
| Answer selection | `src/app/manager/catalog/context.rs` and `context/ranking.rs` | Application-ranked bounded evidence with explicit omitted-source telemetry. Ordering is advisory; the evidence manifest remains authoritative. |
| Answer validation | `src/app/manager/catalog/decode.rs` and Manager output validation | Citations must resolve to supplied evidence. Conflict claims require distinct supplied sources. Evidence text, `AGENTS.md`, worker output and chat do not change policy or supply approval. |

The relevant trust boundaries are the public capability catalog, a pinned
published-spec ref, each separately identified code repository/ref, private
installation state, and the app-owned Manager policy/tool boundary. Repository
content and retrieved documents are untrusted data. The #451 strict Manager
role and brokered-tool boundary remains authoritative: an evidence answer
cannot invoke host shell, approve work, bypass QA, publish specs, or expand
repository access. Public product discovery is isolated from private workspace
state.

## Existing integration evidence

The source already contains cross-boundary coverage, including:

- `app::manager::investigation::tests::verified_manager_broker_excerpt_passes_baseline_provenance_and_freshness_validation`: a Python secondary repository is profiled, queried through the broker, handed to Manager as verified evidence, and rejected when provenance is stale or mismatched.
- `app::manager::catalog::context_tests::published_navigation_routes_to_documents_without_selecting_agents_files`: navigation selects a pinned document, while the index's instruction text is not treated as an answer source.
- `core::investigation_broker::tests::broker_returns_hash_verified_evidence_and_reuses_replayed_requests`: source pinning, receipt verification, idempotent replay, wrong-repository denial, budgets and stale-source behavior.
- `core::repository_knowledge::tests::repository_isolation::profiles_isolate_repository_identity_and_branch_ref`: independent repository and branch identity remain part of the profile key.
- Manager output and context tests cover citation validation, evidence ordering/budgets, uncertainty and tool-role limits.

These tests prove application boundaries and service composition. They do not
measure an authenticated language model's semantic answer accuracy.

## Reproducible question set

[`tests/fixtures/manager-knowledge-questions-v1.json`](../tests/fixtures/manager-knowledge-questions-v1.json)
contains 44 stable questions across product support, repository architecture,
implementation behavior, product intent, local work status, revision drift,
multi-repository separation, contradictions, uncertainty and authorization.
The fixtures include Kool.ad/e Rust plus synthetic Python API and TypeScript
web repositories. The non-product fixtures deliberately include conflicting
README/manifest facts, an intent-versus-source difference, incomplete docs and
untrusted navigation text. They contain no customer data or credentials.

Run the corpus/scorer contract tests with:

```sh
python3 -m unittest scripts/tests/test_manager_knowledge_evaluation.py
```

For a model run, save one JSON object per question to both a `before` and `after`
JSONL file. Each result must use the same question IDs and include the actual
provider usage/cost, preparation/runtime latency, profile refresh/cache and
omission measurements, plus independently assigned `grounded_correct`,
`unsupported_implementation_claim`, `unsupported_approval_claim` and
`asked_human` judgments. `judgment_source` accepts only `human` or
`deterministic_rule`; model self-grading is rejected. Score with:

```json
{"question_id":"Q01","stage":"after","judgment_source":"human","grounded_correct":true,"unsupported_implementation_claim":false,"unsupported_approval_claim":false,"asked_human":false,"latency_ms":820,"input_tokens":1200,"output_tokens":180,"cached_input_tokens":400,"cost_usd":0.0007,"profile_cache_hits":1,"profile_refreshes":0,"scanned_bytes":2048,"omitted_sources":0}
```

```sh
python3 scripts/evaluations/manager_knowledge.py --before before.jsonl --after after.jsonl
```

The report compares before/after correctness on identical questions, reports
per-category accuracy, the unnecessary question rate for evidence-answerable
cases, critical unsupported claims, p50/p95 latency, token/cost totals, profile
cache hits/refreshes, scanned bytes and omitted sources. The after target is at
least 90% grounded correctness and zero unsupported critical implementation or
approval claims. The scorer fails closed for incomplete/duplicate question
sets or non-reviewer judgments.

## Current evaluation status

The corpus shape and scorer contract pass their unit tests. No before/after
provider result files have been collected for this revision, so no accuracy,
question-rate, latency, cost, or provider-smoke result is claimed here. Those
measurements remain required before #483's integration/evaluation acceptance
can be marked complete. The automated Git, profile, broker, Manager-boundary,
and security tests remain separate evidence and are not substituted for a
provider run.
