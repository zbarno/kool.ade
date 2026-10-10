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
        product_directory: ".koolade-packet/planning/product".into(),
        project_config_path: ".koolade-packet/config/project.md".into(),
        user_message: "Let's talk auth".into(),
        conversation: vec![],
        elided_messages: 0,
        overview: Overview::default(),
        spec_markdown: Some("# Demo spec\n".into()),
        product_index: None,
        active_feature: None,
        selected_documents: vec![],
        selected_areas: vec![],
        open_items_markdown: "# Open Items\n".into(),
        config_markdown: "## Stakeholders\n".into(),
        imports: vec![],
        mcp_summary: None,
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
