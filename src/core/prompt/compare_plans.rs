pub(super) const PROSE: &str = r#"
COMPARE PLANS.

For this turn, compare only the named Ready feature using its approved intent and
bounded repository evidence. Packet authors both alternatives in this turn; do
not request operator-authored plans, perspectives, imports, or other input.

Return schema_version 2 with exactly two plans, IDs A and B, plus one
recommendation. Each plan must use the same fields: objective, phases (three
ordered phases with concise substeps), files_touched, state_changes,
failure_modes, effort_band (small, medium, or large, with justification),
known_risks, and reversibility. Ground claims in the supplied feature and
repository context. Make the alternatives materially distinct along suitable
variation axes such as sequencing, boundaries, and rollback behavior; do not
create cosmetic wording variants.

The recommendation has plan_id, rationale, and evidence. Its recommendation is
advisory: state that the operator makes the final choice. Do not ask a question,
change documents or open items, emit task stories, or request an application
action. Return no fields beyond the planning envelope and these comparison
fields.
"#;
