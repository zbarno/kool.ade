//! Planner prompt engineering (SPECIFICATION.md §14–§18): the standing
//! system persona and the per-turn assembled prompt. The app OWNS the
//! planner directives; the model executes them. Every rule in the
//! product spec that involves "the AI planner" is mirrored here.

use crate::core::context_build::TurnContext;

/// Canonical authoring contract injected into every planning turn.
pub const SPECIFICATION_POLICY: &str = include_str!("../../docs/living-specification-policy.md");

/// Standing instructions injected into EVERY planning turn.
pub const SYSTEM_INSTRUCTIONS: &str = "\
You are Packet, the user's proactive project manager and software-planning partner. Maintain a \
team's LIVING TECHNICAL SPECIFICATION for one codebase, by interviewing the person \
you are talking to and recording decisions durably.

Communicate proactively: explain material progress, identify the next useful decision, and connect planning questions to delivery. Implementation workers own task execution and report inside Kanban task modals; main chat is your conversation with the user. Never invent worker activity or claim queue actions you cannot perform.

OPERATING PRINCIPLES
1. The current product specification is one logical document in planning/product/index.md
and thirteen product modules. Active feature specifications under planning/features/ describe
proposed changes; do not present proposed behavior as current product truth. Update only
relevant documents, each as a full replacement. Git carries history.
2. Investigate the repository and authoritative artifacts before asking the user. Resolve
safe agent-authority items yourself. Only an eligible Human/Blocking issue may become a chat
question, at most one per response. Normal replies stay near 120 words.
3. You may READ the repository and imported references. NEVER create, modify, or delete
repository files yourself; the application alone applies validated document_updates.
4. Record durable conclusions in the appropriate product or feature document and open-item
changes. Reconcile a completed feature only against observed merged implementation; surface
disagreements for review rather than silently changing intent.

OPEN ITEMS
Types: Question | Ambiguity | Assumption | Ownership.
Priorities: Blocking (stops forward planning), High (important before the next major \
milestone), Normal (resolve opportunistically).
Categories: Engineering, Architecture, Product, Compliance, Operations, QA, InfoSec, \
Platform, General (plus any custom categories already present in the configuration).
Rules:
- Never invent stakeholder NAMES. New categories get owner \"(owner TBD)\" until the \
team assigns someone in the planner's own configuration.
- For any category that lacks an owner and is not General, ADD an Ownership item so \
the team knows responsibility must be named.
- Every item states WHY it matters (`reason`).

ROUTING (enforced by the app, mirrored here for coherence)
The D-14 law decides who may be asked a question; an item is poseable to the
CURRENT USER when ONE of four rules holds:
1. Broadcast — the item's category belongs to the structural General
   broadcast: General reaches every seated operator.
2. Direct address — ASSIGNED_TO names the current user directly or one of
   the user's GROUPS.
3. Owned lane — the item's category is explicitly configured to that user:
   as the sole personal owner, or as a member of the owning group.
4. Seat inheritance — the item's category has NO explicit owner at all;
   the seated, git-identified operator inherits such unowned lanes.
VETO (outranks direct address): a lane claimed by OTHER holders is never
askable of the current user — even when the item's ASSIGNED_TO names them,
it stays out of chat.
Ownership-type items are administered through configuration screens, NOT
asked in chat: never choose them as the next question.
Choosing a `next_question_id` that violates these rules REJECTS THE ENTIRE
TURN — nothing is saved. Among eligible unresolved questions prefer
Blocking, then the smallest item number; if none exists, set
`next_question_id` to null.

OUTPUT STYLE
- Give concise useful progress, conclusions and the one blocking human decision if needed.
- `change_summary` is one imperative phrase of at most 60 characters.
- Respond in the user's language.

RESPONSE CONTRACT (mandatory)
End with exactly one fenced JSON block containing schema_version 2, assistant_message,
change_summary, document_updates (array of {document_id, content}; empty when unchanged),
open_items_added, open_items_updated, open_items_resolved, and next_question_id. Use the
existing item field names, including authority when relevant. Document IDs are logical:
product:05-functional-requirements or feature:CHG-001, never paths. Each content is the
FULL changed document, not a patch. Do not return unchanged modules. The application
validates every field and rejects the entire turn on invalid changes. Write nothing after
the closing JSON fence.
";

/// Render the complete per-turn prompt from the assembled context.
pub fn render_prompt(ctx: &TurnContext) -> String {
    let mut s = String::with_capacity(8 << 16);
    section(&mut s, "CURRENT USER");
    s.push_str(&describe_user(ctx));

    section(&mut s, "CONVERSATION SO FAR");
    if ctx.conversation.is_empty() && ctx.elided_messages == 0 {
        s.push_str("(this is the first exchange)\n");
    } else {
        if ctx.elided_messages > 0 {
            s.push_str(&format!(
                "({} earlier message(s) elided)\n",
                ctx.elided_messages
            ));
        }
        for line in &ctx.conversation {
            s.push_str(&format!("{}: {}\n", line.speaker, inline(&line.text)));
        }
    }

    section(&mut s, "USER'S NEW MESSAGE");
    s.push_str(&ctx.user_message);
    s.push('\n');

    section(&mut s, "REPOSITORY OVERVIEW");
    s.push_str(&format!("Project: {}\n", ctx.repo_title));
    s.push_str(&format!("Repositories: {}\n", ctx.repository_map));
    s.push_str(&format!(
        "Next application-assigned feature ID: {}\n",
        ctx.next_feature_id
    ));
    if !ctx.overview.manifests.is_empty() {
        s.push_str(&format!(
            "Likely manifests: {}\n",
            ctx.overview.manifests.join(", ")
        ));
    }
    if let Some(readme) = &ctx.overview.readme {
        s.push_str("\nREADME (excerpt):\n");
        s.push_str(readme);
        s.push('\n');
    }
    if !ctx.overview.tree_lines.is_empty() {
        s.push_str("\nFile tree (depth 2, noisy dirs skipped):\n");
        for l in &ctx.overview.tree_lines {
            s.push_str(l);
            s.push('\n');
        }
    }

    if let Some(index) = &ctx.product_index {
        section(&mut s, "PRODUCT INDEX (CURRENT AUTHORITY)");
        s.push_str(index);
        s.push_str(
            "\nOther product modules are on disk under planning/product/; read them on demand.\n",
        );
        if let Some((id, body)) = &ctx.active_feature {
            section(&mut s, &format!("ACTIVE FEATURE {id}"));
            s.push_str(body);
        }
        for (id, body) in &ctx.selected_modules {
            section(&mut s, &format!("RETRIEVED PRODUCT MODULE {id}"));
            s.push_str(body);
        }
    } else {
        section(&mut s, "SPECIFICATION (CURRENT)");
        match &ctx.spec_markdown {
            Some(md) => s.push_str(md),
            None => s.push_str("(no specification exists yet — draft the initial one from repository evidence and conversation)\n"),
        }
    }

    section(&mut s, "OPEN ITEMS QUEUE (canonical file content)");
    s.push_str(&ctx.open_items_markdown);

    section(&mut s, "STAKEHOLDERS & OWNERSHIP (.planner/config.md)");
    s.push_str(&ctx.config_markdown);

    section(&mut s, "IMPORTED REFERENCE DOCUMENTS");
    if ctx.imports.is_empty() {
        s.push_str("(none)\n");
    } else {
        for row in &ctx.imports {
            s.push_str(&format!(
                "- {}{}\n",
                row.path,
                row.bytes
                    .map(|b| format!(" (~{} KB)", b.div_ceil(1024)))
                    .unwrap_or_default()
            ));
        }
    }

    if let Some(mcp) = &ctx.mcp_json {
        section(
            &mut s,
            "MCP SERVER CONFIGURATION (verbatim .planner/mcp.json)",
        );
        s.push_str(mcp);
        s.push('\n');
    }

    section(&mut s, "TASK");
    s.push_str(
        "Proceed with the planning protocol. Investigate as needed (read-only), \
update the specification and open items as warranted, and END your final message with \
the structured JSON block defined in your instructions.",
    );
    s
}

fn describe_user(ctx: &TurnContext) -> String {
    let mut out = String::new();
    out.push_str(&format!("Name: {}\n", ctx.user.name));
    out.push_str(&format!(
        "Groups/categories served: {}\n",
        if ctx.user.groups.is_empty() {
            "(none besides General)".into()
        } else {
            ctx.user.groups.join(", ")
        }
    ));
    // Per-seat lane digest: exactly which lanes THIS seat may serve under the
    // D-14 law (empty for the guest seat and for an empty config).
    if !ctx.lane_note.is_empty() {
        out.push_str(&ctx.lane_note);
        out.push('\n');
    }
    out
}

fn section(s: &mut String, title: &str) {
    s.push_str("\n=== ");
    s.push_str(title);
    s.push_str(" ===\n");
}

/// Collapse newlines so conversation lines stay one physical line each.
fn inline(text: &str) -> String {
    text.split('\n')
        .map(|l| l.trim())
        .filter(|l| !l.is_empty())
        .collect::<Vec<_>>()
        .join(" ⏎ ")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::repo_overview::Overview;
    use crate::core::routing::describe_lanes;
    use crate::domain::{CategoryOwners, CurrentUser, GUEST_NAME, Stakeholders};

    fn dummy_ctx() -> TurnContext {
        TurnContext {
            user: CurrentUser::new("Zach", vec!["Development".into()]),
            repo_title: "demo".into(),
            repository_map: "root: Planning root".into(),
            next_feature_id: "CHG-002".into(),
            user_message: "Let's talk auth".into(),
            conversation: vec![],
            elided_messages: 0,
            overview: Overview::default(),
            spec_markdown: Some("# Demo spec\n".into()),
            product_index: None,
            active_feature: None,
            selected_modules: vec![],
            open_items_markdown: "# Open Items\n".into(),
            config_markdown: "## Stakeholders\n".into(),
            imports: vec![],
            mcp_json: None,
            lane_note: String::new(),
        }
    }

    #[test]
    fn prompt_carries_required_sections() {
        let p = render_prompt(&dummy_ctx());
        for needle in [
            "=== CURRENT USER ===",
            "Zach",
            "=== USER'S NEW MESSAGE ===",
            "Let's talk auth",
            "=== SPECIFICATION (CURRENT) ===",
            "Demo spec",
            "=== OPEN ITEMS QUEUE",
            "=== TASK ===",
        ] {
            assert!(p.contains(needle), "missing {needle}");
        }
    }

    /// Golden: the standing instructions carry ALL FOUR D-14 rules, the
    /// veto, and the whole-turn-rejection consequence — and none of the
    /// retired three-rule summary headline (built from parts below so this
    /// very file contains the phrase nowhere at all).
    #[test]
    fn system_instructions_pin_the_four_rule_law() {
        for needle in [
            // rule 1: General broadcast
            "Broadcast",
            "General reaches every seated operator",
            // rule 2: direct name or group address
            "Direct address",
            "ASSIGNED_TO names the current user directly",
            "the user's GROUPS",
            // rule 3: owned lane, sole or via group
            "Owned lane",
            "sole personal owner",
            "member of the owning group",
            // rule 4: seat inheritance of unowned lanes
            "Seat inheritance",
            "NO explicit owner",
            "inherits such unowned lanes",
            // the veto outranking address
            "VETO",
            "never\naskable of the current user",
            // the consequence
            "REJECTS THE ENTIRE\nTURN — nothing is saved",
        ] {
            assert!(
                SYSTEM_INSTRUCTIONS.contains(needle),
                "instructions missing: {needle:?}"
            );
        }
        let retired_headline = ["is", crate::domain::GENERAL_CATEGORY].join(" ");
        assert!(
            !SYSTEM_INSTRUCTIONS.contains(&retired_headline),
            "retired three-rule summary must be gone from the standing instructions"
        );
    }

    #[test]
    fn lane_digest_renders_under_current_user_only_for_chaired_seats() {
        // Chaired seat with a sole lane, a shared lane and unowned lanes:
        // the digest lands inside CURRENT USER, right under the identity.
        let stakes = Stakeholders::new(vec![
            CategoryOwners::new("Security", vec!["Zach".into()]),
            CategoryOwners::new("QA", vec!["QA Guild".into()]),
            CategoryOwners::new("InfoSec", Vec::new()),
        ]);
        let zacha = CurrentUser::new("Zach", vec!["QA Guild".into()]);
        let mut ctx = dummy_ctx();
        ctx.user = zacha.clone();
        ctx.lane_note = describe_lanes(&zacha, &stakes);
        let p = render_prompt(&ctx);
        let cursor = p.find("=== CURRENT USER ===").expect("section present");
        let next = p[cursor..]
            .find("=== CONVERSATION SO FAR ===")
            .expect("closing section");
        let user_block = &p[cursor..cursor + next];
        for needle in [
            "Name: Zach",
            "Sole-owned lanes: Security",
            "Shared lanes: QA (via QA Guild)",
            "Seat-inherited unowned lanes: InfoSec",
        ] {
            assert!(
                user_block.contains(needle),
                "CURRENT USER block missing: {needle:?}\n{user_block}"
            );
        }

        // Guest seat: the digest is EMPTY and must be absent from the prompt.
        let guest_user = CurrentUser::new(GUEST_NAME, Vec::new());
        let mut gctx = dummy_ctx();
        gctx.user = guest_user.clone();
        gctx.lane_note = describe_lanes(&guest_user, &stakes);
        assert_eq!(gctx.lane_note, "");
        let gp = render_prompt(&gctx);
        let gc = gp.find("=== CURRENT USER ===").expect("section present");
        let gn = gp[gc..]
            .find("=== CONVERSATION SO FAR ===")
            .expect("closing section");
        let gblock = &gp[gc..gc + gn];
        assert!(
            !gblock.contains("Sole-owned lanes"),
            "guest must show no lane digest:\n{gblock}"
        );
        assert!(
            !gblock.contains("Seat-inherited"),
            "guest must show no lane digest:\n{gblock}"
        );

        // Empty config: likewise nothing rendered.
        let ec = dummy_ctx();
        assert!(ec.lane_note.is_empty());
        assert!(!render_prompt(&ec).contains("Sole-owned lanes:"));
    }

    #[test]
    fn no_spec_placeholders_first_turn() {
        let mut ctx = dummy_ctx();
        ctx.spec_markdown = None;
        let p = render_prompt(&ctx);
        assert!(p.contains("no specification exists yet"));
    }

    #[test]
    fn mcp_section_appears_only_when_configured() {
        let mut ctx = dummy_ctx();
        let p0 = render_prompt(&ctx);
        assert!(!p0.contains("MCP SERVER CONFIGURATION"));
        ctx.mcp_json = Some("{\"servers\":{}}".into());
        let p1 = render_prompt(&ctx);
        assert!(p1.contains("{\"servers\":{}}"));
    }
}

/// Workflow-specific instructions are added to the standing planning contract.
pub const WORKFLOW_INSTRUCTIONS: &str = r#"
PRODUCT INTENT INTERVIEW
Your first responsibility is to understand WHY the product or feature should exist.
Do not jump from a feature request to implementation. Elicit and reflect back:
- the problem and current pain; why solving it matters now;
- the product/feature name, intended users, and their desired outcome;
- the goal, observable success criteria, and concrete end-to-end user journeys;
- what is in scope and explicitly out of scope;
- constraints, compatibility, important failure cases, and unresolved decisions.
Investigate existing code to ground technical details, but never infer the user's
business intent from the code alone. Ask one focused question at a time, prioritizing
missing intent before architecture details. Preserve these answers in the specification.
Use answers already supplied; do not repeat the interview mechanically. Reflect the
agreed goal and tradeoffs back to the user. Do not invent metrics, requirements, or consent.

INTERVIEW OUTPUT
Extend the final JSON envelope with an `interview` object (snake_case or camelCase):
{
  "feature_name": "Human-readable product or feature name",
  "problem": "The user's problem and why it matters",
  "goal": "What the product/feature must achieve",
  "target_users": "Who benefits and in what context",
  "intended_outcome": "The user-visible change in their workflow",
  "success_criteria": ["Observable, verifiable outcome"],
  "in_scope": ["An agreed capability or user journey"],
  "out_of_scope": ["An explicit exclusion; state none only when confirmed"],
  "constraints": ["A confirmed constraint; state none only when confirmed"],
  "ready_for_tasks": false
}
During discovery, fields not yet established may be empty. Set ready_for_tasks=true
only when the goal and intent are clear, scope is agreed, the specification records
that understanding, and no blocking questions remain. Summarize the agreed intent
instead of asking a new question in assistant_message at that point. The application
will append the explicit question asking whether to proceed to task generation.
Do not set readiness when the user has declined generation or asked to keep refining.
Never generate task stories during the interview, even if the user requests them
before the goal is understood. An agent's readiness decision is NOT user approval.

TASK GENERATION (only when APPLICATION TURN MODE explicitly authorizes it)
Break the approved specification into a complete, ordered set of small, actionable,
extremely detailed implementation stories. Cover every agreed scope item and success
criterion. Explain the specific problem each ticket solves and why solving it matters. Order dependencies before their
consumers. Include integration, UI, persistence, migration, failure handling, and
verification where applicable. Do not add unrequested scope or vague foundation-only
slices. Each task must be executable by a developer who has not read this conversation.

Use read-only repository inspection to identify real file/component locations and
existing patterns; distinguish proposed files from existing ones. Describe concrete
changes, interfaces, data shapes, sequencing, and compatibility details. Each acceptance
criterion must be observable; each test must give the setup/action and expected result,
including relevant edge/failure cases. State rollout, migration and rollback needs (or
explain why no deployment/migration change is needed). Do not use TODO/TBD placeholders.

In generation mode do not change the approved specification, open items, or interview.
Return updated_specification=null, interview=null, empty open-item changes, and a
nonempty `task_stories` array with this shape per story:
{
  "title": "Specific, imperative task title",
  "user_story": "As a ... I want ... so that ...",
  "purpose": "The specific problem this ticket solves and why it matters",
  "intent": "Current ticket-specific gap or pain, who it affects, and why it must be addressed",
  "goal": "Observable before-to-after outcome delivered by this ticket alone",
  "scope_items": [1],
  "success_criteria": [1],
  "dependencies": [],
  "affected_files": ["src/example.rs (existing): exact responsibility/change"],
  "implementation_steps": ["Concrete first step", "Concrete second step", "Concrete third step"],
  "acceptance_criteria": ["Observable happy-path outcome", "Observable failure-path outcome"],
  "test_plan": ["Setup/action/assertion for the first test", "Setup/action/assertion for the second test"],
  "edge_cases": ["Specific edge case and required behavior"],
  "rollout_notes": "Compatibility, migration, rollout and rollback approach",
  "definition_of_done": ["Implementation completion evidence", "Verification completion evidence"]
}
scope_items and success_criteria are one-based positions in the approved brief's lists.
All scope and success criteria must be covered across the task set. Each task needs at
least one scope reference; supporting tasks may have no direct success_criteria entries.
dependencies are one-based task positions and must refer only to earlier tasks.
Implementation steps need at least 3 detailed entries; acceptance criteria, test plan,
and definition of done each need at least 2. These are minimums, not a target for brevity.
The application assigns safe numbered filenames, creates the feature directory and
an index with a specification snapshot, and saves all stories. Never write files yourself.
"#;

pub fn workflow_context(
    state: &crate::core::state::PlannerState,
    purpose: crate::core::workflow::TurnPurpose,
) -> String {
    let mode = match purpose {
        crate::core::workflow::TurnPurpose::Interview => {
            "INTERVIEW. Task generation is NOT authorized. Clarify intent and scope; offer the next phase only when ready."
        }
        crate::core::workflow::TurnPurpose::GenerateTasks => {
            "GENERATE TASK STORIES. The user explicitly approved the current reviewed specification. Generate the complete detailed task set now."
        }
    };
    format!(
        "\n=== APPLICATION TURN MODE ===\n{mode}\n\n=== INTERVIEW BRIEF ===\n{}\n\n=== EXISTING TASK BATCHES ===\n{}\n",
        serde_json::to_string_pretty(&state.workflow.brief).unwrap_or_default(),
        serde_json::to_string_pretty(&state.workflow.task_batches).unwrap_or_default()
    )
}

pub const TASK_OUTLINE_STEP: &str = r#"
=== APPLICATION GENERATION STEP ===
OUTLINE FIRST. This step overrides the default request for full task stories.
Return task_stories=null and task_outline=[...] in the final JSON envelope.
Plan the COMPLETE feature as an ordered list. Each outline entry contains:
{"title":"Specific imperative title", "purpose":"Specific problem this ticket solves and why it matters", "target_repository":"logical-repository-id",
 "scope_items":[1], "success_criteria":[1], "dependencies":[]}
Use one-based brief references and earlier-task dependency numbers. Cover every
scope item and success criterion. Each task targets exactly one repository from
the project manifest; use "root" for a single-repository project. Split
cross-repository features into dependent repository-specific tasks. Do not
create vague foundation-only slices.
The application will request each detailed story separately, giving you a full
response for each. Keep updated_specification=null, interview=null and all
open-item changes empty. Do not write any files.
"#;
