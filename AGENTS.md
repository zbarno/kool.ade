# Intent

Kool.ad/e is a native desktop application for planning software projects through conversation. It maintains a living specification, routes open questions and decisions, and organizes implementation stories and work in a Git repository. Kool.ad/e teases out ambiguity and drives clear focused specifications.

# Confirmation

- Before starting work explain the goal of the task you are about to work on and have the user confirm that your understanding is correct

## Completion rule

Do not claim the task is complete until:

- The implementation satisfies the original goal
- All acceptance criteria are met
- The independent validation pass has completed
- Blocking reviewer findings have been addressed
- Relevant tests, checks, or manual validation have passed
- Any remaining known limitations are documented

# OUTPUT

- Always be conscise. Do not provide detailed respones unless explicitly asked to
- When your work is complete Provide a short summary of the work, what was changed, what work remains, recommended
  next actions
  Example (Do not include the output in a code block):
  ```
  # SUMMARY
  ## Findings
  Short description of your analysis

  ## WHAT CHANGED
  List of changes that were made.  These should be a concise listing, not overly descriptive

  ## REVIEW FINDINGS AND REMDIATIONS
  - Confirmation that the `CRITICAL VALIDATION REQUIREMENTS` steps were followed and performed by an independent agent
  - List of findings and the remediation status

  ## WHAT REMAINS
  List of reamining work to complete the goal.  These should be a concise listing, not overly descriptive

  ## NEXT STEPS
  The next most logical task based on the current state of the session

  ```

# Always

- Keep each source module under about 300 lines. Split growing modules into focused submodules before they become substantially larger.
- Use Rust's directory module layout: declare a module in `foo.rs` and place its child modules in `foo/` (for example, `core.rs` and `core/implementation.rs`).
- Treat `.koolade-packet/` as the only live shared Kool.ad/e project-artifact root; legacy roots are migration inputs or historical records only.
- Keep Markdown human-readable and derive workflow behavior from structured metadata and state.
- Treat plan recommendations as advisory; only explicit operator adoption selects a plan and unlocks approval.
- Before an agent marks a task complete, run the repository quality gates: `cargo +1.98.1 fmt --all --check`, `cargo +1.98.1 test --locked --all-targets -- --test-threads=1`, and `cargo +1.98.1 clippy --locked --all-targets -- -D warnings`.
- All three quality gates must pass before marking the task complete or committing. If a gate cannot run or fails, report the result and leave the task incomplete until it is resolved.

# Never

- Never create or use a `mod.rs` file. Use the directory module layout described above instead.
