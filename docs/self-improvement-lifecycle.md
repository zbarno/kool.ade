# Self-improvement lifecycle and trust boundaries

Self-improvement is a regular Kool.ad/e feature workflow whose target is the canonical Kool.ad/e repository. An opportunity recommendation is not authorization. A persisted user Pursue decision may create a planning goal, but implementation still waits for requirement clarification, user plan confirmation, and an independent plan review. The ordinary task, claim, execution, review, runtime QA, specification reconciliation, and publication controls remain authoritative.

## Isolation and publication

Implementation runs in a task-specific clone pinned to the approved source SHA and destination branch. The executor receives only that clone and the capability required for the current task. It receives no host credentials, Docker socket, or publication authority. The controller publishes through the ordinary claim-fenced Git workflow after it verifies the reviewed diff and approvals. A PR does not imply merge, installation, or activation.

The controller persists lifecycle evidence against the exact plan and implementation hashes. Optimistic revisions reject stale transitions. Code review, runtime QA, and specification receipts must identify the implementation hash. Publication, merge, and activation are distinct states; activation requires an observed readiness probe for the installed version. This projection grants no execution, merge, or deployment permission.

## Self-hosting threat model

Treat candidate changes to authorization, provider launch, Docker policy, reviewer prompts, state persistence, and update logic as untrusted until independently reviewed and negatively tested. Candidate code cannot forge user decisions or reviewer receipts. Reviewers use fresh contexts and cannot publish. UI QA uses a separately built fixture application and isolated state, never the running user application. No linked user project is mutated while developing Kool.ad/e itself.

Opportunities discovered during an improvement remain proposals. They cannot recursively create implementation tasks without a separate user decision. Self-improvement work uses a bounded queue so it cannot starve active user goals. User-local logs and sensitive project evidence are not copied into the public product repository or PR.

## Current model boundary

`SelfImprovementRunV1` validates the product lifecycle evidence and expected revision for each transition. It is a domain record only: persistence, actor authentication, clone allocation, reviewer receipt verification, and publication must be supplied by the existing application-owned lifecycle services before any transition is trusted in production. A model response is never sufficient evidence for one of those actions.
