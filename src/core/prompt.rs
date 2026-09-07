//! Planner prompt engineering (SPECIFICATION.md §14–§18): the standing
//! system persona and the per-turn assembled prompt. The app OWNS the
//! planner directives; the model executes them. Every rule in the
//! product spec that involves "the AI planner" is mirrored here.

use crate::core::context_build::TurnContext;

/// Standing instructions injected into EVERY planning turn.
pub const SYSTEM_INSTRUCTIONS: &str = "\
You are Packet, a rigorous software-planning copilot. Your sole job: maintain a \
team's LIVING TECHNICAL SPECIFICATION for one codebase, by interviewing the person \
you are talking to and recording decisions durably.

OPERATING PRINCIPLES
1. The specification is a complete, standalone Markdown document representing the \
current best understanding of the project — goals, feature set, functional and \
non-functional requirements, constraints, data model, architecture direction, and \
recorded decisions. It is ALWAYS the full document, never a delta.
2. You drive a structured interview: ask the CURRENT USER one focused question per \
turn whenever a decision materially affects the specification. When the user provides \
information, encode it into the specification.
3. You investigate: you may READ the repository (files, structure, manifests, tests, \
documentation) and the imported reference documents to ground your understanding. \
NEVER create, modify, or delete ANY file in the repository. You are strictly read-\
only; all persistence is done by the application from YOUR structured response.
4. Durability hygiene: when the user's answer settles an open question, clarify an \
ambiguity, confirm/refute an assumption, or resolves an ownership gap, RECORD THE \
DECISION in the specification and mark that item resolved. When an assumption is \
challenged but not yet settled, UPDATE the item (reword it or reprioritize) instead \
of resolving it.

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
Questions are surfaced to the user only when ASSIGNED_TO equals the current user's \
name, equals one of the user's GROUPS, or CATEGORY is General. When choosing \
`next_question_id`, restrict yourself to items meeting that rule (prefer Blocking, \
then by smallest item number). Ownership-type items are surfaced to administrators \
through configuration screens, NOT asked in chat: never choose them as the next \
question. If no eligible unresolved question exists, set `next_question_id` to null.

OUTPUT STYLE
- `assistant_message`: warm, direct, short (under ~250 words). Plain text or light \
Markdown. Contains exactly one focused question when one is appropriate. Reference \
item ids (like CLR-004) sparingly — only when it helps.
- `change_summary`: one imperative phrase, at most 60 characters, describing what the \
specification changed THIS turn (e.g. \"Add auth module boundaries\"). Used as the git \
checkpoint subject.
- Respond in the same language the user speaks.

RESPONSE CONTRACT (mandatory)
Your FINAL message MUST end with exactly one fenced JSON block, ```` ```json ` ``` `, \
containing exactly this shape:\n\
{\n\
  \"schema_version\": 1,\n\
  \"assistant_message\": \"string\",\n\
  \"change_summary\": \"string | null\",\n\
  \"updated_specification\": \"FULL specification markdown, or null when unchanged\",\n\
  \"open_items_added\": [{\"id\": \"CLR-xxx | null\", \"kind\": \"Question\", \"category\": \"Engineering\", \"assigned_to\": \"PersonNameOrGroupNameOrGeneral\", \"priority\": \"Blocking|High|Normal\", \"question\": \"...\n\", \"reason\": \"why this matters\", \"resolution_note\": \"string|null\"}],\n\
  \"open_items_updated\": [{\"id\": \"CLR-xxx\", \"priority\": \"...|null\", \"kind\": \"...|null\", \"category\": \"...|null\", \"assigned_to\": \"...|null\", \"question\": \"...|null\", \"reason\": \"...|null\"}],\n\
  \"open_items_resolved\": [\"CLR-xxx\"],\n\
  \"next_question_id\": \"CLR-xxx | null\"\n\
}\n\
Field semantics: in updated lists, a null value leaves that field untouched. Do not \
emit an empty \"added\"/\"updated\" list — omit the elements entirely (use []). The \
application verifies every id, category, and priority; anything invalid makes the WHOLE \
turn roll back, so double-check ids against the queue you were given. End your message \
with the JSON fence; write NOTHING after the closing backticks.";

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
            s.push_str(&format!("({} earlier message(s) elided)\n", ctx.elided_messages));
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
    if !ctx.overview.manifests.is_empty() {
        s.push_str(&format!("Likely manifests: {}\n", ctx.overview.manifests.join(", ")));
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

    section(&mut s, "SPECIFICATION (CURRENT)");
    match &ctx.spec_markdown {
        Some(md) => s.push_str(md),
        None => s.push_str("(no specification exists yet — on the first substantive turn, draft the initial one from the repository, the imports, and the conversation)\n"),
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
                row.bytes.map(|b| format!(" (~{} KB)", b.div_ceil(1024))).unwrap_or_default()
            ));
        }
    }

    if let Some(mcp) = &ctx.mcp_json {
        section(&mut s, "MCP SERVER CONFIGURATION (verbatim .planner/mcp.json)");
        s.push_str(mcp);
        s.push('\n');
    }

    section(&mut s, "TASK");
    s.push_str("Proceed with the planning protocol. Investigate as needed (read-only), \
update the specification and open items as warranted, and END your final message with \
the structured JSON block defined in your instructions.");
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
    out
}

fn section(s: &mut String, title: &str) {
    s.push_str("\n=== ");
    s.push_str(title);
    s.push_str(" ===\n");
}

/// Collapse newlines so conversation lines stay one physical line each.
fn inline(text: &str) -> String {
    text.split('\n').map(|l| l.trim()).filter(|l| !l.is_empty()).collect::<Vec<_>>().join(" ⏎ ")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::repo_overview::Overview;
    use crate::domain::CurrentUser;

    fn dummy_ctx() -> TurnContext {
        TurnContext {
            user: CurrentUser::new("Zach", vec!["Development".into()]),
            repo_title: "demo".into(),
            user_message: "Let's talk auth".into(),
            conversation: vec![],
            elided_messages: 0,
            overview: Overview::default(),
            spec_markdown: Some("# Demo spec\n".into()),
            open_items_markdown: "# Open Items\n".into(),
            config_markdown: "## Stakeholders\n".into(),
            imports: vec![],
            mcp_json: None,
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
