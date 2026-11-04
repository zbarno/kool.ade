use super::*;
use crate::core::repo_overview::Overview;
use crate::domain::CurrentUser;

fn dummy_ctx() -> TurnContext {
    TurnContext {
        user: CurrentUser::new("Zach"),
        repo_title: "demo".into(),
        repository_map: "root: Planning root".into(),
        next_feature_id: "CHG-002".into(),
        product_directory: "planning/product".into(),
        project_config_path: "config/project.md".into(),
        user_message: "Let's talk auth".into(),
        conversation: vec![],
        elided_messages: 0,
        overview: Overview::default(),
        spec_markdown: Some("# Demo spec\n".into()),
        product_index: None,
        active_feature: None,
        active_feature_is_local_proposal: false,
        local_spec_proposals: vec![],
        local_spec_proposal_base: None,
        selected_documents: vec![],
        selected_areas: vec![],
        open_items_markdown: "# Open Items\n".into(),
        config_markdown: "## Current User\nName: Operator\n".into(),
        imports: vec![],
        mcp_summary: None,
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

/// CHG-003: every turn's standing instructions carry the unlabeled
/// reply-tail digest emission contract. Each positive needle lies inside
/// ONE physical source line of the constant (continuation backslashes
/// glue wrap lines with no separator), so a careless re-wrap breaks the
/// pin by design.
#[test]
fn system_instructions_carry_the_unlabeled_digest_contract() {
    for needle in [
        // Heading; uppercased on purpose to avoid the pre-existing
        // lowercase lane-digest wording in this file, and split into
        // two adjacent literals so the heading keeps its exactly-once
        // occurrence in this source file (the const line itself).
        concat!("REPLY-TAIL ", "DIGEST (display convention"),
        "awaits the user's input",
        "one line containing only ---,",
        "one to five short bullet lines,",
        "each beginning with '- '",
        "'Option 1', 'Option 2',",
        "remain always valid",
        "carries no digest.",
    ] {
        assert!(
            SYSTEM_INSTRUCTIONS.contains(needle),
            "instructions missing: {needle:?}"
        );
    }
    // The RESPONSE CONTRACT closing survives, and its sentinel line
    // occurs exactly once: the digest paragraph is APPENDED after it,
    // never interleaved or reordering the const.
    assert!(
        SYSTEM_INSTRUCTIONS.contains("Write nothing after"),
        "RESPONSE CONTRACT closing lost"
    );
    assert_eq!(
        SYSTEM_INSTRUCTIONS
            .matches("the closing JSON fence.")
            .count(),
        1,
        "'the closing JSON fence.' must occur exactly once"
    );
    // Const-tail integrity: the digest paragraph is the LAST section of
    // the standing instructions and still ends with its final newline.
    assert!(
        SYSTEM_INSTRUCTIONS.ends_with("the digest helps the operator skim, nothing more.\n"),
        "digest paragraph must end the standing instructions"
    );
    // The retired task-mode line grammar never entered the standing
    // instructions; only the new unlabeled digest is taught here.
    assert!(
        !SYSTEM_INSTRUCTIONS.contains("Your next step"),
        "legacy emission order leaked into the standing instructions"
    );
}

/// Human planning questions are routed to the local user without team seats.
#[test]
fn system_instructions_route_human_questions_to_local_user() {
    for needle in [
        "Human questions belong to the local user",
        "Agent-owned items are not asked as user questions",
        "Open prerequisites block dependent items",
        "Choose only an existing, open, unblocked Human item",
    ] {
        assert!(
            SYSTEM_INSTRUCTIONS.contains(needle),
            "instructions missing: {needle:?}"
        );
    }
    assert!(!SYSTEM_INSTRUCTIONS.contains("ASSIGNED_TO names the current user"));
    assert!(!SYSTEM_INSTRUCTIONS.contains("the user's GROUPS"));
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
    assert!(!p0.contains("MCP SERVER SUMMARY"));
    ctx.mcp_summary = Some("Configured MCP server names (commands hidden):\n- search".into());
    let p1 = render_prompt(&ctx);
    assert!(p1.contains("MCP SERVER SUMMARY"));
    assert!(p1.contains("- search"));
}

#[path = "tests/hostile_persona.rs"]
mod hostile_persona;
#[path = "tests/persona_assembly.rs"]
mod persona_assembly;
