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
