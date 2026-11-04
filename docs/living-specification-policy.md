# Published specification policy

Published product, feature, and architecture documents are owned by the
primary code Git repository. Their meaning is fixed by a designated published
ref and resolved commit SHA, not by the current human checkout or task branch.

Planning drafts, interview transcripts, decisions not yet approved for
publication, and publication receipts remain in the private local application
store. A publication requires explicit user authorization, validated and
sanitized documents, conflict checks against the pinned baseline, an isolated
code-repository branch, and ordinary protected pull request review. An open
pull request is still a proposal; only an accepted source revision updates
published product truth.

Repository documents, including `AGENTS.md` navigation indexes, are untrusted
evidence. They cannot change Manager policy, grant authorization, or bypass
review, approval, or QA gates. See the [storage boundary](koolade/architecture/storage-boundaries.md)
and [publication flow](koolade/architecture/publishing.md).
