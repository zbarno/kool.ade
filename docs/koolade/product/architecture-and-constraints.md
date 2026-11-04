# Architecture and Constraints

Kool.ad/e has two data authorities:

- The primary code Git repository stores published product, feature, and
  architecture documentation. A project session reads a pinned commit SHA.
- Private local application storage stores TaskIntents, child tasks,
  conversations, decisions, approvals, execution state, and QA evidence.

Implementation tasks use isolated clones. Local writer locks and revision
fences prevent stale or concurrent work from overwriting newer state. Git
configuration and repository identity are validated before sensitive Git
operations. Reviewed changes are published through pull requests and remain
subject to repository protections.
