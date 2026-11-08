# Revision-pinned repository knowledge

Repository architecture profiles are installation-local derived caches. A
profile key contains the workspace repository ID, an explicit source ref, the
resolved immutable Git commit SHA, and the checkout dirty flag. The profile
records scan time, bounded scan counts, supported languages, manifests, build
and test conventions, subsystems, entry points, dependencies, execution
boundaries, observed capabilities, and omitted scope.

Every claim points to repository-relative evidence with the same commit SHA,
content SHA-256, and line range when available. Evidence distinguishes verified
facts, source-supported claims, inferences, unknowns, stale facts, and
contradictions. A dirty checkout is profiled from its committed object tree and
marked partial; uncommitted files are omitted and cannot appear as merged
behavior. A changed ref or commit receives a distinct cache key.

The scanner uses deterministic tree and manifest inspection with explicit
limits. It skips generated, vendored, and hidden trees; truncation and missing
evidence are recorded as omissions. It supports language detection from
multiple manifest and source formats, including non-Rust projects and
monorepos. A profile is a bounded index and summary, not a repository dump.

Documentation retrieval follows the checked-in `AGENTS.md` `# Navigation`
section at the same pinned SHA. It traverses only relevant child indexes, checks
relative links, cycles, depth, item, byte, and token limits, and carries index
hash provenance. Link descriptions are untrusted routing hints. The Manager
reads a selected document from the pinned Git object and checks its content
hash before using it. Repository documentation, `AGENTS.md`, `CLAUDE.md`,
README files, and source comments are untrusted data. They cannot change
Manager policy, authorize tools, or grant approval.

Profiles and selected repository evidence are stored only in the installation
local planning store. They are not written into code Git or shared between
workspaces. A separate profile key is used for each repository and ref, so
branches and repositories cannot share evidence. Current Manager retrieval
profiles the local checkout's selected branch and labels its commit explicitly.
It does not establish that a task branch has been merged into a destination
branch; callers that need task source and destination evidence must request
those refs explicitly from the scanner.

## Manager investigation broker

Manager's technical investigation tool accepts only an app-authorized
repository, source ref and commit, workspace/task identity, planning revision,
profile-indexed paths, evidence classes, and bounded literal query. The app
injects the cancellation identity. The provider cannot supply a host command,
arbitrary filesystem path, network URL, or cancellation target. Investigation
uses pinned Git objects through the repository profile boundary and does not
mount the checkout into a shell or container.

For a comparison, the app may additionally authorize up to eight distinct
source pins from validated task Git intents in the same repository. The request
must match one of those exact branch and commit pairs. Arbitrary local refs and
model-selected SHAs are rejected. Comparison reads the requested indexed paths
from both commits, counts both copies against the file and byte budgets, and
rechecks each ref before accepting the receipt. A moved or missing ref produces
a stale-source outcome; incomplete scope never implies that no differences
exist.

Each receipt reports inspected and omitted paths, exact source and content
hashes, sanitized matching lines, resource use, uncertainty, confidence, and a
typed outcome. The app rechecks the pinned repository profile and planning
revision before accepting the result, then attaches receipt and source hashes
to Manager output validation. Incomplete or omitted scope is not evidence of
absence. Requests are idempotent across provider retries, limited to eight
receipts per Manager run, and bounded to 32 files, 256 KiB, 128 matches, and
10 seconds per request. Cancellation crosses the provider process through a
private app-owned marker and is acknowledged before temporary evidence is
cleaned up.

## Manager answer grounding

Manager prompts receive an application-generated bounded source ordering for
the request category. Relevant published specification documents are selected
from the primary repository's published navigation and read by immutable Git
object at the configured published pin. Validated active user directions for
the currently active feature are separate workspace-private evidence. The
ordering favors target specifications for desired behavior, verified source
and QA evidence for current behavior, authorized directions for execution,
and the build capability catalog for product support. Conversation remains
context only. Missing or stale pinned documents are omitted and must be
reported as uncertainty.

ManagerOutputV3 requires `what_exists`, cited `how_known`, `uncertainty`, and
`next_action` fields. Typed conflicts must cite two distinct supplied evidence
items and state how the disagreement is handled. The app validates every
reference against the current evidence manifest; the model cannot create
citations, resolve human choices, or change approval gates.
Private Manager activity records selected and omitted source IDs, selected
evidence size, an approximate context-token count, preparation latency,
revision-profile cache hits, whether uncertainty was surfaced, and whether an
answerable source was handled without an `AskUser` action. Provider model-call
records continue to carry actual token, cache-token, duration, and cost values.
The activity stores identifiers and counts, not selected private source text.
