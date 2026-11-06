# Product capability catalog

The app compiles `assets/capabilities/product-capabilities.json` as its public, build-owned product capability catalog. Each entry has a stable ID, user-facing description, maturity, invocation paths, constraints, platform and provider prerequisites, limitations, source/test references, and an optional app-owned readiness probe ID. The current build version, full source commit, and catalog SHA-256 accompany catalog snapshots.

Manager lookup tokenizes a bounded user goal, ranks at most five catalog entries deterministically, and returns `Unknown` when no entry matches. An empty result is not evidence that a capability is absent. A capability marked implemented in the build is not host-ready. `VerifiedReady` requires exactly one readiness result bound to that capability's registered probe and the running build commit; unavailable, stale, or conflicting results do not verify readiness.

For provider routing, the workspace Manager lookup can use the selected Manager route's persisted local discovery snapshot. Fresh negative authentication, role, model, or profile evidence is reported as implemented but not ready; stale or missing discovery stays unknown. This snapshot does not launch a provider or confirm live model-service availability. Provider settings and readiness are workspace-private and never enter the public product scan.

The product-only opportunity scan receives the sanitized public catalog and designated product contract in an app-owned empty working directory. It does not receive workspace lookup results, local repositories, task data, user settings, approvals, or conversations. Workspace-specific capability lookup is separate and its result is private to that workspace prompt.

Catalog tests pin the stable IDs and registered readiness-probe pairs, check every source/test path, and bound matching and output. Feature changes must update the catalog and these checks together; removed or renamed IDs require an explicit compatibility decision.
