//! Deterministic fixture preparer for the binding §12 exit demonstration
//! (companion: `docs/exit-demo-runbook.md`, Step 0).
//!
//! Invoke from the Packet source checkout:
//!
//! ```sh
//! cargo run --offline --example prepare_exit_demo [OPTIONS] [DEST]
//! ```
//!
//! It authors a small, self-contained git fixture repository (`tinypipe`,
//! a stdlib-only Python line pipeline) carrying naive, deliberately flawed
//! planning state, then SELF-VERIFIES the result through the app's own
//! strict loader (`packet::core::state::PlannerState::load`) before
//! exiting 0. The prepared fixture is the stage for the seven-outcome
//! walkthrough:
//!
//! * `planning/specification.md` — a naive draft with EXACTLY three seeded
//!   wrong claims, each refutable from the fixture code;
//! * `planning/open-items.md` — CLR-001 (Product/Ambiguity/Normal),
//!   CLR-002 (QA/Question/High), CLR-003 (InfoSec/Question/Normal; InfoSec
//!   is sole-owned, so that lane can never be posed to the chair), emitted
//!   by the app's own `items_io::serialize`;
//! * `.planner/config.md` — the seven default categories, InfoSec
//!   sole-owned by "Mira Voss", all others memberless, and deliberately NO
//!   `## Current User` block so the seated identity flows from the
//!   git-identity path alone (D-14/D-23).
//!
//! Determinism: every byte written is a fixed literal (no timestamps), so
//! two consecutive preparations into fresh directories yield byte-identical
//! `planning/`, `.planner/`, `README.md`, `pyproject.toml`, and `tinypipe/`
//! contents. Only git commit dates and SHAs vary between runs — the
//! runbook explicitly excludes them from the determinism claim.
//!
//! Guards: before ANYTHING is written, a DEST that exists and is non-empty,
//! or that already carries a `.git`, is refused with a fixed message and
//! exit 1. Destinations are canonicalized up front so the emptiness check,
//! the `.git` check, and the printed manifest all refer to one real
//! directory. If the git phase or self-verification fails, the partially
//! built directory is left in place (never mutated further) for
//! inspection; the planning files on disk are nonetheless complete,
//! strict-parser-valid artifacts, so no parse-breaking residue exists.
//!
//! Exit codes: 0 on success (and on `--help`), 1 on guard refusal or any
//! preparation/self-verification failure, 2 on usage errors.
//!
//! NFR-7: links only the `packet` library plus std; Cargo auto-discovers
//! `examples/`, so `Cargo.toml` is deliberately untouched.

use std::ffi::OsString;
use std::path::{Path, PathBuf};
use std::process::{Command, ExitCode};

use packet::artifacts::{
    CONFIG_FILE, OPEN_ITEMS_FILE, SPEC_FILE, atomic_write, config_io, items_io,
};
use packet::core::state::PlannerState;
use packet::domain::{
    CategoryOwners, DEFAULT_CATEGORIES, ItemKind, OpenItem, Priority, Stakeholders,
};

// ---------------------------------------------------------------------------
// Fixed values
// ---------------------------------------------------------------------------

const DEFAULT_DEST: &str = "packet-exit-fixture";
const DEFAULT_SEAT_USER: &str = "Zachary Barno";
const DEFAULT_SEAT_EMAIL: &str = "zbarno@gmail.com";
const SEED_SUBJECT: &str = "chore: seed tinypipe fixture";
const SOLE_OWNED_CATEGORY: &str = "InfoSec";
const SOLE_OWNER: &str = "Mira Voss";
const RUNBOOK_POINTER: &str = "docs/exit-demo-runbook.md";
const GUARD_REFUSAL: &str =
    "destination already populated or is a git repo — remove it or pass a different path";

/// The three seeded wrong claims. Each must survive self-verification in
/// the loaded spec, and each is refutable by reading the fixture code:
/// claim 1 by `tinypipe/core.py::Pipeline.run`, claims 2–3 by
/// `tinypipe/cli.py::main`.
const SEEDED_WRONG_CLAIMS: [&str; 3] = ["reverse registration order", "gunzip", "1 MiB"];

const USAGE: &str = "\
prepare_exit_demo — deterministic §12 exit-demo fixture preparer (Packet)

Usage:
  prepare_exit_demo [OPTIONS] [DEST]

Arguments:
  DEST                    Destination directory for the fixture repository
                          (default: ./packet-exit-fixture, relative to the cwd)

Options:
  --user <NAME>           Chair seat name written into the fixture's LOCAL
                          git config (default: Zachary Barno)
  --email <ADDR>          Chair seat address written into the fixture's LOCAL
                          git config (default: zbarno@gmail.com)
  -h, --help              Print this help and exit

Value rule: in the space form (--user NAME), a value that looks like an
option (leading '-') is refused as a usage error; use --user=NAME to pass
dash-leading names.

Behavior:
  Refuses (exit 1) when DEST is already populated or is a git repo;
  exits 2 on usage errors; self-verifies the seeded planning state through
  the app's own strict loader before exiting 0.";

/// Fixture README — the ground-truth account the naive spec contradicts.
const TINYPY_README: &str = "\
# tinypipe

A stdlib-only Python line pipeline for small toy projects. Lines enter on
standard input, pass through a chain of registered transforms, and leave
on standard output.

## Behavior

- Transforms register themselves in code at startup (decorator style).
- The chain executes strictly in registration order.
- Input is plain UTF-8; no compression codecs are involved.
- Output is written in full, with no size cap.
- Exit codes: 0 success, 1 transform raised, 2 bad usage.

## Layout

- `tinypipe/core.py` — the `Pipeline` class and the process-wide registry.
- `tinypipe/cli.py` — the command-line entry point.

## Usage

    python -m tinypipe.cli < input.txt > output.txt
";

/// Five lines, name/version focused — picked up by the per-turn survey's
/// root-manifest scan (`src/core/repo_overview.rs`).
const PYPROJECT_TOML: &str = "\
[project]
name = \"tinypipe\"
version = \"0.1.0\"
description = \"A stdlib-only Python line pipeline.\"
requires-python = \">=3.9\"
";

/// Ground truth for seeded claim 1: transforms run IN REGISTRATION ORDER.
const CORE_PY: &str = "\
\"\"\"Core of tinypipe: ordered chains of line transforms.

Transforms register on a Pipeline and run one after another, strictly in
REGISTRATION ORDER (first registered runs first). Nothing sorts, shuffles
or reverses the chain. Standard library only — in fact no imports at all.
\"\"\"


class Pipeline:
    \"\"\"A named, ordered line-transform chain.\"\"\"

    def __init__(self):
        # The list order IS the execution order; nothing re-orders it.
        self._steps = []

    def register(self, name, func):
        \"\"\"Append ``func(line) -> line`` under ``name``; usable as decorator.\"\"\"
        self._steps.append((name, func))
        return func

    @property
    def names(self):
        \"\"\"Names in registration order (a debugging aid).\"\"\"
        return [name for name, _func in self._steps]

    def run(self, lines):
        \"\"\"Feed every line through every transform, in registration order.\"\"\"
        current = list(lines)
        for _name, func in self._steps:
            current = [func(line) for line in current]
        return current


# The process-wide pipeline; modules register their transforms on it.
PIPELINE = Pipeline()
";

/// Ground truth for seeded claims 2–3: no decompression, no output cap.
const CLI_PY: &str = "\
\"\"\"Command-line entry point for tinypipe.

Reads ALL of standard input as plain UTF-8 text — no gzip or any other
decompression, ever — feeds the lines through the registered transforms
in registration order, and writes the COMPLETE result to standard output
with no cap on output size.

Exit codes:
    0  success
    1  a transform raised an exception
    2  bad usage (spare arguments or non-UTF-8 input)
\"\"\"
import sys

from tinypipe.core import PIPELINE


def main(argv=None):
    argv = sys.argv if argv is None else argv
    if len(argv) != 1:
        sys.stderr.write(\"usage: python -m tinypipe.cli\\n\")
        return 2
    try:
        raw = sys.stdin.read()  # plain UTF-8 read; nothing is decompressed
    except UnicodeDecodeError as exc:
        sys.stderr.write(f\"tinypipe: stdin is not valid UTF-8 ({exc})\\n\")
        return 2
    try:
        out_lines = PIPELINE.run(raw.splitlines())
    except Exception as exc:  # a transform blew up mid-stream
        sys.stderr.write(f\"tinypipe: transform failed ({exc})\\n\")
        return 1
    if out_lines:
        sys.stdout.write(\"\\n\".join(out_lines) + \"\\n\")  # uncapped, in full
    return 0


if __name__ == \"__main__\":
    raise SystemExit(main())
";

/// The naive seeded specification: deliberately shallow sections the
/// interview can grow, plus EXACTLY the three refutable wrong claims
/// (marked in bold below). Every other statement matches the fixture code.
const NAIVE_SPEC: &str = "\
# tinypipe

_Naive draft spec — written from memory, before reading the code._

## Overview

tinypipe is a stdlib-only Python line pipeline. Text enters on standard
input, moves through a series of registered transforms, and leaves on
standard output.

## Runtime model

Transforms are registered in code at startup. They execute in **reverse registration order**, so the newest transform sits closest to the output.

## Input handling

Input is plain text — unless it is compressed: the CLI transparently
**gunzips** standard input whenever the leading bytes carry the gzip magic
number, so plain pipes and gzipped pipes both work with the same command.

## Output limits

To protect slow terminals, total output is **capped at 1 MiB**; anything
beyond the cap is silently dropped.

## Exit codes

0 success, 1 internal failure, 2 misuse.

## Testing

_Not started yet._
";

// ---------------------------------------------------------------------------
// Invocation parsing
// ---------------------------------------------------------------------------

struct Args {
    dest: PathBuf,
    user: String,
    email: String,
}

enum ParsedInvocation {
    Run(Args),
    Help,
}

/// Positional DEST (default `packet-exit-fixture` relative to the cwd),
/// `--user`/`--email` (defaults: the D-23 chair seat), `--help`. Whitespace
/// user/email after trim and unknown options are usage errors (exit 2).
fn parse_args(tokens: impl IntoIterator<Item = OsString>) -> Result<ParsedInvocation, String> {
    let mut dest: Option<PathBuf> = None;
    let mut user: Option<String> = None;
    let mut email: Option<String> = None;
    let mut help = false;
    let mut awaiting_value: Option<&'static str> = None;

    for token in tokens {
        let tok = token.to_string_lossy().into_owned();
        if let Some(flag) = awaiting_value.take() {
            if tok.len() > 1 && tok.starts_with('-') {
                return Err(format!(
                    "{flag} takes a value, but received what looks like an option: '{tok}'"
                ));
            }
            match flag {
                "--user" => user = Some(tok),
                "--email" => email = Some(tok),
                _ => unreachable!("awaiting_value only tracks known flags"),
            }
            continue;
        }
        if let Some(value) = tok.strip_prefix("--user=") {
            user = Some(value.to_string());
        } else if let Some(value) = tok.strip_prefix("--email=") {
            email = Some(value.to_string());
        } else {
            match tok.as_str() {
                "-h" | "--help" => help = true,
                "--user" => awaiting_value = Some("--user"),
                "--email" => awaiting_value = Some("--email"),
                _ if tok.len() > 1 && tok.starts_with('-') => {
                    return Err(format!("unknown option '{tok}'"));
                }
                _ => {
                    if dest.is_some() {
                        return Err(format!("more than one positional DEST supplied ('{tok}')"));
                    }
                    dest = Some(PathBuf::from(tok));
                }
            }
        }
    }
    if let Some(flag) = awaiting_value {
        return Err(format!("{flag} requires a value (ran out of arguments)"));
    }
    if help {
        return Ok(ParsedInvocation::Help);
    }
    let user = user.unwrap_or_else(|| DEFAULT_SEAT_USER.to_string());
    let email = email.unwrap_or_else(|| DEFAULT_SEAT_EMAIL.to_string());
    if user.trim().is_empty() {
        return Err("--user must not be blank (received whitespace only)".into());
    }
    if email.trim().is_empty() {
        return Err("--email must not be blank (received whitespace only)".into());
    }
    Ok(ParsedInvocation::Run(Args {
        dest: dest.unwrap_or_else(|| PathBuf::from(DEFAULT_DEST)),
        user: user.trim().to_string(),
        email: email.trim().to_string(),
    }))
}

// ---------------------------------------------------------------------------
// Destination guard (fires before ANY filesystem write of ours)
// ---------------------------------------------------------------------------

enum SecureDestination {
    Ready(PathBuf),
    Populated,
    Io(std::io::Error),
}

/// Canonicalize DEST and prove it is (or was just created as) empty and
/// not a git repo. An existing file, a populated directory, or a `.git`
/// entry all refuse the run before the preparer touches anything.
fn secure_destination(raw: &Path) -> SecureDestination {
    if raw.exists() {
        if !raw.is_dir() {
            return SecureDestination::Populated;
        }
        if raw.join(".git").exists() {
            return SecureDestination::Populated;
        }
        // An unenumerable directory (e.g. permission-denied) is refused
        // conservatively: refusal is cheaper than building atop unknown
        // state.
        match std::fs::read_dir(raw) {
            Ok(mut rd) => {
                if rd.any(|entry| entry.is_ok()) {
                    return SecureDestination::Populated;
                }
            }
            Err(..) => return SecureDestination::Populated,
        }
        match std::fs::canonicalize(raw) {
            Ok(canon) => SecureDestination::Ready(canon),
            Err(e) => SecureDestination::Io(e),
        }
    } else {
        if let Err(e) = std::fs::create_dir_all(raw) {
            return SecureDestination::Io(e);
        }
        match std::fs::canonicalize(raw) {
            Ok(canon) => SecureDestination::Ready(canon),
            Err(e) => SecureDestination::Io(e),
        }
    }
}

// ---------------------------------------------------------------------------
// Seed construction — pure logic, IO separated (unit-testable in principle;
// deliberately NOT #[test]-annotated here so the example adds no tests to
// the NFR-8 census (192 unit + 1 integration on this tree).
// ---------------------------------------------------------------------------

/// The three seeded items, queue-ordered (High first, then ID-break ties).
fn seed_items() -> Vec<OpenItem> {
    let mut items = vec![
        OpenItem::new(
            "CLR-001".into(),
            Priority::Normal,
            ItemKind::Ambiguity,
            "Product".into(),
            None,
            "Does the pipeline promise line-count fidelity — must every transform \
             return exactly one line per input line, or may a transform drop lines \
             or fan one out into many?"
                .into(),
            "The draft spec says lines flow \"through a series of transforms\" \
             without fixing what a transform may do to cardinality, and the v0.2 \
             --dry-run design hangs on the answer."
                .into(),
        ),
        OpenItem::new(
            "CLR-002".into(),
            Priority::High,
            ItemKind::Question,
            "QA".into(),
            None,
            "Which behaviors deserve executable tests first — especially, what \
             observable outcome separates exit code 1 (a transform raised) from \
             exit code 2 (bad usage or non-UTF-8 input)?"
                .into(),
            "tinypipe/cli.py advertises three distinct exit codes, but no test \
             strategy exists yet; QA needs an acceptance definition before the \
             v0.2 cut."
                .into(),
        ),
        OpenItem::new(
            "CLR-003".into(),
            Priority::Normal,
            ItemKind::Question,
            SOLE_OWNED_CATEGORY.into(),
            None,
            "If tinypipe ever emits telemetry (diagnostics or audit trails) about \
             processed lines, which sinks are allowed, and what must telemetry do \
             when a line is suspected of carrying a secret?"
                .into(),
            "Defines the surface any security review would have to cover; the \
             InfoSec lane is sole-owned, so this item can only ever be posed to \
             its named owner, never to the chair."
                .into(),
        ),
    ];
    items_io::sort_queue(&mut items);
    items
}

/// Seven default categories, InfoSec sole-owned by the fictional
/// "Mira Voss" (a live D-14 sole-ownership case), all others memberless.
/// The seat identity deliberately does NOT enter this file: omitting the
/// `## Current User` block forces the git-identity seating path, and the
/// seat travels as the fixture's local git config instead. The parameters
/// are retained so the preparer's contract makes that routing explicit.
fn seed_config(_user: &str, _email: &str) -> String {
    let entries = DEFAULT_CATEGORIES
        .iter()
        .map(|category| {
            let members = if *category == SOLE_OWNED_CATEGORY {
                vec![SOLE_OWNER.to_string()]
            } else {
                Vec::new()
            };
            CategoryOwners::new((*category).to_string(), members)
        })
        .collect();
    config_io::serialize(&config_io::PlannerConfig {
        user: None,
        stakeholders: Stakeholders::new(entries),
    })
}

/// The naive tinypipe specification (fixed literal, no timestamps).
fn seed_spec_text() -> String {
    NAIVE_SPEC.to_string()
}

// ---------------------------------------------------------------------------
// Phases
// ---------------------------------------------------------------------------

/// The four project literals, each under 40 lines so the per-turn survey
/// (depth-2 tree + root manifests) sees the whole project.
fn write_project_files(root: &Path) -> Result<(), String> {
    let files: [(&str, &str); 4] = [
        ("README.md", TINYPY_README),
        ("pyproject.toml", PYPROJECT_TOML),
        ("tinypipe/core.py", CORE_PY),
        ("tinypipe/cli.py", CLI_PY),
    ];
    for (rel, text) in files {
        atomic_write(&root.join(rel), text)
            .map_err(|e| format!("write phase failed at '{rel}': {e}"))?;
    }
    Ok(())
}

/// The three planning artifacts, all produced by the app's OWN serializers
/// (NFR-2 atomic writes throughout).
fn write_planning_seed(root: &Path, args: &Args) -> Result<(), String> {
    let seed = seed_items();
    let items_md = items_io::serialize(&seed);
    let config_md = seed_config(&args.user, &args.email);
    let spec_md = seed_spec_text();
    let targets: [(&str, &str); 3] = [
        (SPEC_FILE, &spec_md),
        (OPEN_ITEMS_FILE, &items_md),
        (CONFIG_FILE, &config_md),
    ];
    for (rel, text) in targets {
        atomic_write(&root.join(rel), text)
            .map_err(|e| format!("write phase failed at '{rel}': {e}"))?;
    }
    Ok(())
}

/// Run one git invocation by argument array (no shell interpolation,
/// mirroring `src/core/gitops.rs`). Errors name the failing invocation
/// (including its working directory, for copy-paste repro) and carry its
/// captured stderr; launch failures name the invocation too, so a missing
/// git is diagnosed precisely.
fn git_run(cwd: &Path, args: &[&str]) -> Result<String, String> {
    let display = format!("git -C {:?} {}", cwd, args.join(" "));
    let out = Command::new("git")
        .arg("-C")
        .arg(cwd)
        .args(args)
        .output()
        .map_err(|e| format!("failed to launch '{display}': {e} — git never started, so no stdout or stderr was captured (is git on PATH?)"))?;
    let stderr = String::from_utf8_lossy(&out.stderr).trim().to_string();
    if !out.status.success() {
        let code = out.status.code().unwrap_or(-1);
        let detail = if stderr.is_empty() {
            String::new()
        } else {
            format!("\ncaptured stderr:\n{stderr}")
        };
        return Err(format!("'{display}' exited with code {code}{detail}"));
    }
    Ok(String::from_utf8_lossy(&out.stdout).trim().to_string())
}

/// init -b main → LOCAL seat identity → stage all → single seed commit.
/// The commit is authored by the SEAT (local config), never by Packet
/// Planner, keeping the post-connect checkpoint chain attributable to app
/// turns alone (protects outcome (e)).
fn git_phase(root: &Path, args: &Args) -> Result<(), String> {
    git_run(root, &["init", "-q", "-b", "main"])?;
    git_run(root, &["config", "user.name", args.user.as_str()])?;
    git_run(root, &["config", "user.email", args.email.as_str()])?;
    git_run(root, &["add", "-A", "--", "."])?;
    git_run(root, &["commit", "-q", "-m", SEED_SUBJECT])?;
    Ok(())
}

/// Fail-loud self-verification, run on EVERY invocation. The finished repo
/// must reload through the app's real loader with the exact seed vectors,
/// the exact stakeholder shape, a live set of seeded claims, and the seat
/// actually sitting in the repo and on the seed commit.
fn verify_phase(root: &Path, args: &Args) -> Result<(String, String), Vec<String>> {
    let state = match PlannerState::load(root) {
        Ok(state) => state,
        Err(e) => {
            return Err(vec![format!(
                "PlannerState::load({}) failed: {e}",
                root.display()
            )]);
        }
    };
    let seed = seed_items();
    let mut problems: Vec<String> = Vec::new();

    // (i) The written queue must round-trip through the strict parser.
    let items_path = root.join(OPEN_ITEMS_FILE);
    match std::fs::read_to_string(&items_path) {
        Ok(written) => match items_io::parse(&written) {
            Ok(parsed) if parsed == seed => {}
            Ok(parsed) => problems.push(format!(
                "open-items.md round-trip: strict parser yielded {} item(s) that diverge from the {}-item seed vector",
                parsed.len(),
                seed.len()
            )),
            Err(e) => problems.push(format!(
                "open-items.md round-trip: strict parser rejected the written file: {e}"
            )),
        },
        Err(e) => problems.push(format!("cannot read back {}: {e}", items_path.display())),
    }

    // (ii) The loaded queue must equal the constructed seed field-for-field.
    if state.items.len() != seed.len() {
        problems.push(format!(
            "queue length: loaded {} item(s), expected {}",
            state.items.len(),
            seed.len()
        ));
    } else {
        for (index, (loaded, wanted)) in state.items.iter().zip(seed.iter()).enumerate() {
            diff_item(index, loaded, wanted, &mut problems);
        }
    }

    // (iii) Config: no Current User block, seven categories, InfoSec as the
    //       ONLY owned one with the single fictitious member.
    if state.config.user.is_some() {
        problems.push(
            "config: a '## Current User' block is present — it must be OMITTED so the seat derives from git alone".into(),
        );
    }
    for category in DEFAULT_CATEGORIES {
        match state.config.stakeholders.find(category) {
            Some(entry) => {
                let wanted_members: Vec<&str> = if *category == SOLE_OWNED_CATEGORY {
                    vec![SOLE_OWNER]
                } else {
                    Vec::new()
                };
                let loaded_members: Vec<&str> = entry.members.iter().map(String::as_str).collect();
                if loaded_members != wanted_members {
                    problems.push(format!(
                        "config: category '{category}' members {loaded_members:?}, expected {wanted_members:?}"
                    ));
                }
            }
            None => problems.push(format!(
                "config: category '{category}' missing from loaded stakeholders"
            )),
        }
    }
    let owned_count = state
        .config
        .stakeholders
        .entries
        .iter()
        .filter(|entry| entry.has_owner())
        .count();
    if owned_count != 1 {
        problems.push(format!(
            "config: expected exactly one owned category ({SOLE_OWNED_CATEGORY}), found {owned_count}"
        ));
    }

    // (iv) Spec loaded, opens with the fixture H1, and still carries all
    //      three seeded wrong claims (their removal is the interview's job).
    match state.spec_text.as_deref() {
        Some(text) if text.starts_with("# tinypipe") => {}
        Some(text) => {
            let head: String = text.chars().take(40).collect();
            problems.push(format!(
                "spec: does not start with '# tinypipe' (starts with {head:?})"
            ));
        }
        None => problems.push("spec: planning/specification.md did not load".into()),
    }
    if let Some(text) = state.spec_text.as_deref() {
        for claim in SEEDED_WRONG_CLAIMS {
            if !text.contains(claim) {
                problems.push(format!(
                    "spec: seeded wrong claim missing from loaded text: '{claim}'"
                ));
            }
        }
    }

    // (v) The seat must actually sit in the new repo — read back from git,
    //     and on the seed commit itself (outcome (e) attribution).
    let read_name = git_run(root, &["config", "user.name"]).unwrap_or_default();
    let read_email = git_run(root, &["config", "user.email"]).unwrap_or_default();
    if read_name != args.user {
        problems.push(format!(
            "git readback: user.name is '{read_name}', expected '{}' — the seat did not land in the repo's local config",
            args.user
        ));
    }
    if read_email != args.email {
        problems.push(format!(
            "git readback: user.email is '{read_email}', expected '{}' — the seat did not land in the repo's local config",
            args.email
        ));
    }
    let author_line = match git_run(root, &["log", "--max-count=1", "--pretty=%an|%ae|%s"]) {
        Ok(line) => line,
        Err(e) => {
            problems.push(format!("seed commit readback failed: {e}"));
            return Err(problems);
        }
    };
    let wanted_author = format!("{}|{}|{}", args.user, args.email, SEED_SUBJECT);
    if author_line != wanted_author {
        problems.push(format!(
            "seed commit: author line '{author_line}', expected '{wanted_author}' — the seed would pollute outcome (e) attribution"
        ));
    }

    if problems.is_empty() {
        Ok((read_name, read_email))
    } else {
        Err(problems)
    }
}

/// Report the first field-level discrepancy at a queue position.
fn diff_item(index: usize, loaded: &OpenItem, wanted: &OpenItem, problems: &mut Vec<String>) {
    let mut mismatch = |field: &str, loaded_v: String, wanted_v: String| {
        if loaded_v != wanted_v {
            problems.push(format!(
                "items[{index}] ({}): {field} loaded={loaded_v:?}, expected {wanted_v:?}",
                wanted.id
            ));
        }
    };
    mismatch("id", loaded.id.clone(), wanted.id.clone());
    mismatch(
        "priority",
        loaded.priority.to_string(),
        wanted.priority.to_string(),
    );
    mismatch(
        "kind",
        loaded.kind.label().to_string(),
        wanted.kind.label().to_string(),
    );
    mismatch("category", loaded.category.clone(), wanted.category.clone());
    mismatch(
        "assigned_to",
        loaded
            .assigned_to
            .clone()
            .unwrap_or_else(|| "(unassigned)".into()),
        wanted
            .assigned_to
            .clone()
            .unwrap_or_else(|| "(unassigned)".into()),
    );
    mismatch("question", loaded.question.clone(), wanted.question.clone());
    mismatch("reason", loaded.reason.clone(), wanted.reason.clone());
    mismatch(
        "status",
        format!("{:?}", loaded.status),
        format!("{:?}", wanted.status),
    );
}

/// The success block: fixture path, the seat READ BACK from the new repo,
/// the three seeded item lines (id|kind|category), runbook entry point.
fn print_manifest(root: &Path, seat_name: &str, seat_email: &str) {
    let seed = seed_items();
    println!();
    println!("EXIT-DEMO FIXTURE — MANIFEST");
    println!("  fixture path : {}", root.display());
    println!(
        "  git seat     : user.name={seat_name} | user.email={seat_email}  (read back from the new repo)"
    );
    println!("  seed commit  : {SEED_SUBJECT}");
    println!("  seeded items :");
    for item in &seed {
        println!(
            "    {}|{}|{}",
            item.id,
            item.kind.label().to_ascii_lowercase(),
            item.category
        );
    }
    println!("  runbook      : {RUNBOOK_POINTER} — begin at Step 1 (Step 0 is complete)");
    println!(
        "Self-verification PASSED: seeded planning state loads through the app's own strict loader."
    );
}

// ---------------------------------------------------------------------------
// Orchestration
// ---------------------------------------------------------------------------

fn prepare(args: &Args) -> ExitCode {
    let root = match secure_destination(&args.dest) {
        SecureDestination::Ready(canon) => canon,
        SecureDestination::Populated => {
            eprintln!("{GUARD_REFUSAL}");
            return ExitCode::FAILURE;
        }
        SecureDestination::Io(e) => {
            eprintln!(
                "ERROR: cannot establish destination '{}': {e}",
                args.dest.display()
            );
            return ExitCode::FAILURE;
        }
    };
    eprintln!("[prep] destination secured: {}", root.display());

    if let Err(stage) = write_project_files(&root) {
        eprintln!(
            "ERROR: preparation stopped after the guard; destination left for inspection (remove it or pass a different path): {stage}"
        );
        return ExitCode::FAILURE;
    }
    eprintln!("[prep] project files written (4 literals)");

    if let Err(stage) = write_planning_seed(&root, args) {
        eprintln!(
            "ERROR: preparation stopped after the guard; destination left for inspection (remove it or pass a different path): {stage}"
        );
        return ExitCode::FAILURE;
    }
    eprintln!("[prep] planning seed written via the app's own serializers");

    if let Err(diagnosis) = git_phase(&root, args) {
        eprintln!(
            "ERROR: preparation aborted in the git phase; destination left for inspection: {diagnosis}"
        );
        return ExitCode::FAILURE;
    }
    eprintln!("[prep] seed commit created by the seat identity");

    match verify_phase(&root, args) {
        Ok((seat_name, seat_email)) => {
            eprintln!("[prep] self-verification passed");
            print_manifest(&root, &seat_name, &seat_email);
            ExitCode::SUCCESS
        }
        Err(problems) => {
            eprintln!(
                "ERROR: self-verification FAILED — the fixture disagrees with its seed ({} discrepanc{}):",
                problems.len(),
                if problems.len() == 1 { "y" } else { "ies" }
            );
            for problem in &problems {
                eprintln!("  - {problem}");
            }
            eprintln!("Remove the destination and re-run.");
            ExitCode::FAILURE
        }
    }
}

fn main() -> ExitCode {
    let tokens: Vec<OsString> = std::env::args_os().skip(1).collect();
    match parse_args(tokens) {
        Ok(ParsedInvocation::Help) => {
            print!("{USAGE}");
            ExitCode::SUCCESS
        }
        Ok(ParsedInvocation::Run(args)) => prepare(&args),
        Err(problem) => {
            eprintln!("usage error: {problem}");
            eprintln!();
            eprintln!("{USAGE}");
            ExitCode::from(2)
        }
    }
}
