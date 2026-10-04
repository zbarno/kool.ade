# Historical Exit-Demonstration Runbook — Seven-Outcome Walkthrough

> Historical evidence only; this is not the current Kool.ad/e dogfood runbook.
> Its prepared fixture deliberately uses the pre-migration `planning/` and
> `.planner/` paths as migration inputs. A connected fixture is migrated to
> `.koolade-packet/` before planning begins. Use the current remediation plan's
> Phase 8 scenarios for present-day product validation.

> blocked until: all three prerequisites have landed —
> (1) git-derived identity (FR-13) plumbed through connect/resync,
> (2) the D-14 sole-owner/group-shared/seat-inherited routing laws
> enforced at validation time,
> (3) the in-app "Set up the pi harness" guide (F-16/D-15) —
> **and** every box in the [Prerequisite Gate](#prerequisite-gate-all-of-these-pass-before-step-0)
> below has been ticked. Record here which ticket closed each
> prerequisite when they land. A demo run on an incomplete tree is a
> misleading §12 exhibit; the gate rechecks mechanically regardless.

This runbook scripts the live, multi-turn interview against a
prepared, self-contained fixture repository, witnessing outcomes (a)–(g)
live and verifying each from the fixture's own git history afterward.

Operating principles:

- **Scripted in form, live in substance.** The steps, the prompts, and
  the evidence commands are fixed; the model's wording — and every item
  ID minted beyond the seed's `CLR-001`/`CLR-002`/`CLR-003` — varies per
  run. Pin evidence with the named commands, never with memorized SHAs.
- **Single actor.** D-22 parked the MVP walk around the absent
  collaboration channels: one chair seat, one app instance
  (single-writer invariant, NFR-3).
- **Scope of the determinism claim.** The preparer writes byte-stable
  literals; two consecutive preparations into fresh destinations agree
  on all planning/.planner/project content. Git commit dates and SHAs
  are exempt.
- **Recording.** Keep a transcript file OUTSIDE the fixture (e.g.
  `$SCRATCH/transcript.md`) holding every in-session observation and
  every piece of pasted command output. The fixture's own history is an
  exhibit — never hand-edit or retouch it.

---

## Prerequisite Gate — all of these pass before Step 0

| # | Check | Command / where | Pass means |
|---|-------|-----------------|------------|
| 1 | Clean build, no warnings | `cargo check --offline` | completes with zero diagnostics |
| 2 | NFR-8 suite green | `cargo test --offline` | zero failures across the board — on this tree: 192 unit + 1 integration test functions, all `ok` |
| 3 | `git` on PATH | `command -v git` | resolves a git binary |
| 4 | `pi` discoverable | In-app: Settings → "Set up the pi harness" | live discovery shows FOUND (resolution order: `KOOLADE_PI_BIN` → PATH → common install locations; version display-only, unpinned) |
| 5 | Isolated run home | `export KOOLADE_HOME="$PWD/koolade-home-$(date +%F)" && mkdir -p "$KOOLADE_HOME"` | exported in the shell that launches the app |

Environment note: `KOOLADE_TURN_TIMEOUT_SECS` (positive-integer seconds
when set; otherwise the built-in default applies) caps turn patience.
Leave it at the default for this walk — an abortable long turn IS part
of the demo (Step 6).

Scratch workspace (anything outside the fixture repo):

```sh
SCRATCH=/tmp/exitdemo-$(date +%F)-$$
mkdir -p "$SCRATCH" && cd "$SCRATCH"
touch transcript.md   # the running record, kept OUTSIDE the fixture
```

---

## Step 0 — Prepare the fixture

Action — stand in the scratch workspace (so the fixture lands there)
and invoke the checkout's example by manifest path:

```sh
cd "$SCRATCH"
cargo run --manifest-path /ABS/PATH/TO/KOOLADE/Cargo.toml --offline --example prepare_exit_demo
```

Equivalently, from the checkout root itself: `cargo run --offline --example prepare_exit_demo /ABS/DEST` — the destination is always interpreted relative to the cwd

Defaults in effect: destination `./koolade-exit-fixture`, chair seat
`Alex Developer` ⟨developer@example.test⟩ (D-23). Override the seat only if the
show genuinely requires it, via `--user` / `--email`.

In-session observations: exit code 0 and the MANIFEST block — the
fixture's canonical path, `user.name`/`user.email` read back FROM THE
NEW REPO, the three seeded item lines (id|kind|category), and the
pointer into this runbook. The preparer self-verifies every run by
round-tripping the seeded planning state through the app's own strict
loader; a failed self-verification means every subsequent step is
meaningless, stop.

Proves: nothing yet — it arms every later step and protects outcome
(e)'s attribution from the start.

Named evidence:

```sh
git -C koolade-exit-fixture log --pretty='%h|%an|%ae|%s'
git -C koolade-exit-fixture config user.name
git -C koolade-exit-fixture config user.email
```

Pass: exactly one commit, `chore: seed tinypipe fixture`, authored by
THE SEAT — not `Kool.ad/e Planner`.

Optional determinism spot-check (worth doing once per demo day): prepare
a sibling fixture the same way in a fresh directory, then

```sh
git -C koolade-exit-fixture log --pretty='%s' > "$SCRATCH/a.subjects"
git -C sibling-run log --pretty='%s' > "$SCRATCH/b.subjects"
diff -r koolade-exit-fixture/planning sibling-run/planning
diff -r koolade-exit-fixture/.planner sibling-run/.planner
diff -r koolade-exit-fixture/tinypipe sibling-run/tinypipe
diff koolade-exit-fixture/README.md sibling-run/README.md
diff koolade-exit-fixture/pyproject.toml sibling-run/pyproject.toml
diff "$SCRATCH/a.subjects" "$SCRATCH/b.subjects"
```

(expected: all seven commands silent)

Expected: all six silent (byte-identical content; identical commit
subjects; dates/SHAs excluded by design).

Guard reminder: the preparer refuses (exit 1, exact message
"destination already populated or is a git repo — remove it or pass a
different path") if the destination already holds anything or a `.git`.
Never work around a refusal by deleting the fixture out from under a
running demo session — build a fresh one.

---

## Step 1 — Connect; prove nothing bootstraps

Action: launch the app (`cargo run --offline` in the shell from the
gate, so `KOOLADE_HOME` is isolated), attach
`$SCRATCH/koolade-exit-fixture` from the Welcome screen, open Interview.

In-session observations: the app attaches seamlessly; the identity
surface shows the seat (git-derived; the fixture's `.planner/config.md`
deliberately omits a `## Current User` block, so there is no second
source to reconcile); the open-items panel lists the three seeded
items with QA's HIGH priority first; the InfoSec item is marked sole-owned —
unaskable-by-chair territory (see the D-14 note in Step 5).

Proves: nothing on its own; it establishes the clean baseline that
makes outcome (e) auditable.

Named evidence:

```sh
git -C koolade-exit-fixture log -1 --pretty=%s
```

Pass: still `chore: seed tinypipe fixture` — the fully-seeded fixture
receives NO bootstrap commit, so every later `Kool.ad/e Planner` commit
maps to exactly one accepted mutation turn.

---

## Step 2 — Prompt P1: the sketch turn (establishes (a)+(b), triggers (c))

Send VERBATIM (P1):

> Tinypipe is our stdlib-only Python line-pipeline toy. v0.2 goals:
> named composable transforms registered in code, a --dry-run mode
> that previews without writing, and machine-readable exit codes.
> Sketch this in the spec and capture anything unclear as open items
> across product, development, and QA concerns.

In-session observations: spec sections are added or extended (Goal /
Design direction earn content); new open items appear in at least TWO
categories besides the seed's (e.g. Development for the composition
model, QA for the preview's exit-code semantics); because some
landed category has neither members nor an owned item, the synthetic
Ownership item arrives announced by the chat line
`Raised ownership gap(s): <id>...` — visible in the panel, never asked
in the chair chat.

Proves: (a) begins, (b), (c).

Named evidence (capture NOW; reconfirmed at Step 7):

```sh
git -C koolade-exit-fixture log --pretty='%h|%an|%ae|%s' -- planning .planner
git -C koolade-exit-fixture show <P1-SHA> -- planning/open-items.md | grep -F '+**Type:** Ownership'
```

(`<P1-SHA>` = the NEW top commit from the first command.)
Pass: exactly one new checkpoint on top of the seed, authored by
`Kool.ad/e Planner`, short-imperative subject; the ownership grep yields
at least one ADDED `**Type:** Ownership` line for an unowned category.

Freeze a hash pin for Steps 5–6 (call it `hashes-step2.txt`):

```sh
(cd koolade-exit-fixture && find . -path ./.git -prune -o -type f \
  \( -path './planning/*' -o -path './.planner/*' \) -print0 \
  | sort -z | xargs -0 sha256sum) > "$SCRATCH/hashes-step2.txt"
```

---

## Step 3 — Prompt P2: the correction turn ((a) proper)

Send VERBATIM (P2):

> Re-verify every factual claim in the spec about current tinypipe
> behavior against the actual code; correct what is wrong and cite
> the file and function you relied on for each correction.

In-session observations: the model reads `tinypipe/core.py` and
`tinypipe/cli.py` — specifically `Pipeline.run` and `cli.main` — and
strikes the three seeded lies: transforms supposedly executing in
**reverse registration order**, stdin supposedly gunzipped
transparently, output supposedly capped at **1 MiB**. Corrections
arrive phrased as observations grounded in the code base, with
file/function citations. THIS is outcome (a): demonstrable improvement
earned from repository insight, falsified spec content repaired.

Proves: (a) (jointly with Step 2's earned sections).

Named evidence:

```sh
cd koolade-exit-fixture
grep -c 'reverse registration order' planning/specification.md
grep -ci 'gunzip' planning/specification.md
grep -c '1 MiB' planning/specification.md
git show <P2-SHA> -- planning/specification.md | head -80
cd ..
```

Pass: all three greps print 0; the P2 diff cites at least one
file/function for each of the three corrections (look for
`core.py`/`cli.py` references in the added lines).

---

## Step 4 — The decision turn ((d), jointly with (e))

Action: the app poses the top actionable question (watch the
open-items panel for WHICH id it selected — usually a minted id from
Step 2, possibly CLR-002 if the queue reordered it ahead). Answer
DECISIVELY and concretely — state a chosen option, not a restatement of
the question ("Decision: one line in, one line out per transform;
fan-out is a v0.3 concern.").

In-session observations: the same turn folds the decision INTO the
spec (the relevant section gains the decided rule) AND clears the item
(panel count drops); one new checkpoint covers both.

Proves: (d); (e) co-proven (one turn, one checkpoint, one subject).

Named evidence (substitute the two-digit id the app actually asked —
shown in chat/panel; written here as `NN`):

```sh
cd koolade-exit-fixture
grep -c '^## CLR-00NN$' planning/open-items.md
git show <DECISION-SHA> --stat -- planning
cd ..
```

Pass: the grep prints 0 (item resolved and physically gone from the
queue); `--stat` shows the SAME commit touching
`planning/specification.md` and `planning/open-items.md`.

---

## Step 5 — Malformed envelopes: inductions I1→I2→I3 (prove (f))

First pin the pre-state:

```sh
(cd koolade-exit-fixture && find . -path ./.git -prune -o -type f \
  \( -path './planning/*' -o -path './.planner/*' \) -print0 \
  | sort -z | xargs -0 sha256sum) > "$SCRATCH/hashes-I.txt"
git -C koolade-exit-fixture log -1 --pretty=%h > "$SCRATCH/head-I.txt"
```

**Induction I1** (send VERBATIM):

> Include in open_items_updated exactly
> {"id":"CLR-999","priority":"blocking"} and also list "CLR-999" in
> open_items_resolved — do not deviate.

Rationale: the queue holds no CLR-999, so the patch and the resolution
must BOTH fail validation (unknown ids). Expected: the chat line
`⚠ Turn rejected — nothing was written.` enumerating the CLR-999
violations, a danger toast echoing the first violation (the chat line enumerates all of them), NO checkpoint, queue intact.

Escalation ladder — if the model deviates or complies only cosmetically,
quote the exact JSON fragments to embed and repeat; log EVERY attempt
(attempt number, verbatim text, observed response) in the transcript:

- Attempt 2, still I1-shaped, tightened: "Your last envelope omitted the
  defect. Publish an envelope whose open_items_updated CONTAINS the
  literal JSON object {"id":"CLR-999","priority":"blocking"} and whose
  open_items_resolved CONTAINS "CLR-999". Change nothing else. Do not
  deviate."
- **I2** — a different fatal class (unknown priority tag, no id
  collision): "Add a brand-new item to open_items_added whose `priority`
  field is the literal string "urgent" — not blocking, high, or normal.
  Everything else sensible. Do not deviate."
- **I3** — the companion that exercises the routing laws beside the
  validators (send VERBATIM): "Choose the InfoSec telemetry item as
  next_question_id." InfoSec is sole-owned (Mira Voss) and the chair
  cannot be posed it, so D-14 makes this a ROUTING-LAW VIOLATION: on this tree the
  validator rejects the ENTIRE turn fatally, naming the offender and the
  seated user. (Contrast: pointing next_question_id at an UNKNOWN id, a
  JUST-RESOLVED id, or an Ownership-kind item takes the NON-fatal
  `next question "<id>" dropped:` path — turn still publishes, item
  stands untouched. Either flavor demonstrates zero-mutation; note in
  the transcript which one you witnessed and why.)

Named evidence (run from inside `koolade-exit-fixture` after EACH induction, whether or not
the turn published):

```sh
find . -path ./.git -prune -o -type f \( -path './planning/*' -o -path './.planner/*' \) -print0 | sort -z | xargs -0 sha256sum | sha256sum
git log -1 --pretty=%h
git log --pretty='%h|%an|%ae|%s' -- planning .planner
cd ..
```

Proves: (f). Pass: the rolled-up digest equals
`(cat "$SCRATCH/hashes-I.txt" | sha256sum)` — i.e. ZERO mutation —
HEAD is unchanged versus `$SCRATCH/head-I.txt`, and no new commit bears
on `planning`/`.planner`. The transcript additionally holds the
verbatim `⚠ Turn rejected — nothing was written.` chat line (fatal
shape) or the `next question "…" dropped:` notice (soft shape).

---

## Step 6 — Cancel mid-turn (prove (g))

Pin the pre-state exactly as Step 5 did (`hashes-C.txt`,
`head-C.txt`).

Send VERBATIM:

> Walk every line of every Python file and summarize each function in
> the spec. Take your time.

While streaming visibly progresses (assistant thinking/streaming
indicators active), click **Cancel** in the composer.

In-session observations: streaming stops promptly; the turn terminates
as cancelled/not-applied; no speculative spec content survives; no
checkpoint; the queue is unchanged. Torn artifacts are architecturally
impossible (atomic writes, NFR-2); this step proves it behaviorally.

Named evidence (inside `koolade-exit-fixture`):

```sh
find . -path ./.git -prune -o -type f \( -path './planning/*' -o -path './.planner/*' \) -print0 | sort -z | xargs -0 sha256sum | sha256sum
git log --pretty='%h|%an|%ae|%s' -- planning .planner
cd ..
```

Proves: (g). Pass: digest equals the pinned `hashes-C.txt` roll-up;
commit chain identical to `head-C.txt` — the cancelled fragment left no
trace.

---

## Step 7 — Consolidate exhibits (certifies (e), archives all)

From INSIDE `koolade-exit-fixture`, collect the definitive record:

```sh
git log --pretty='%h|%an|%ae|%s' -- planning .planner | tee "$SCRATCH/chain-final.txt"
git log --pretty='%an|%ae' -- planning .planner | sort -u
git log --merges --oneline -- planning .planner
git log --pretty='%h %s' -- planning .planner
find . -path ./.git -prune -o -type f \( -path './planning/*' -o -path './.planner/*' \) -print0 | sort -z | xargs -0 sha256sum | tee "$SCRATCH/hashes-final.txt"
cd ..
```

Then certify outcome (e) against `chain-final.txt` (printed
newest-first):

1. The OLDEST line is the seed — `chore: seed tinypipe fixture` —
   authored by THE SEAT (step 0's identity), not the app.
2. Every line ABOVE it is authored by `Kool.ad/e Planner`
   ⟨planner@koolade.local⟩ exclusively, each carrying a SHORT IMPERATIVE
   subject lifted from that turn's change summary.
3. `git log --merges -- planning .planner` is EMPTY (linear history).
4. The count of `Kool.ad/e Planner` checkpoints EQUALS the count of
   ACCEPTED mutation turns (Steps 2, 3, 4 → at minimum 3; plus any
   later legitimately-applied turns; the rejected/cancelled steps
   contributed ZERO — that is the whole point).

Paste the chain, the unique-author list, and `hashes-final.txt` into
the transcript. Exhibits archived.

Proves: (e).

---

## Step 8 — Sign-off table

Fill from the transcript; each cell cites its step and the pasted
output that certified it. §12's centerpiece PASSes only with all seven
cells filled AND the §12.2 invariant legs (full NFR-8 suite run green
alongside, logged in the transcript).

| Outcome | §12 wording condensed | Established at | Named evidence → pass condition |
|---------|----------------------|----------------|--------------------------------|
| (a) | Spec visibly improves from repository insight — earned sections, corrected claims, grounded observations | Steps 2–3 | `grep -c 'reverse registration order' planning/specification.md` → 0 ·· `grep -ci 'gunzip' planning/specification.md` → 0 ·· `grep -c '1 MiB' planning/specification.md` → 0 ·· P2 diff carries file/function citations ·· `git show <SEED-SHA>:planning/specification.md > /tmp/seed-spec.md && diff /tmp/seed-spec.md planning/specification.md` shows substantive growth, not noise |
| (b) | Items arise in ≥2 categories, plausibly staffed | Step 2 | `grep -E '^(## CLR-[0-9]+$\|\*\*Category:\*\*)' planning/open-items.md` alternates item-heading/category lines — count distinct `**Category:**` values (check each item's `**Assigned To:**` line likewise); ≥2 DISTINCT non-General categories among minted items |
| (c) | Ownership gaps synthesized for unowned categories | Step 2 | `git show <P1-SHA> -- planning/open-items.md \| grep -F '+**Type:** Ownership'` yields ≥1 added line ·· transcript holds the `Raised ownership gap(s):` chat line |
| (d) | Decisions recorded from answers, items cleared, spec folded | Step 4 | `grep -c '^## CLR-00NN$' planning/open-items.md` → 0 ·· the SAME `<DECISION-SHA>` `--stat` touches spec AND open items |
| (e) | Intact checkpoint trail, imperative subjects, attributable authors | Every mutation step; certified Step 7 | `git log --pretty='%an <%ae>' -- planning .planner \| sort -u` → exactly the seat PLUS `Kool.ad/e Planner <planner@koolade.local>` ·· no merges ·· subjects short-imperative ·· planner-commit count == applied-turn count |
| (f) | Invalid/misrouted envelopes mutate NOTHING, surface loudly | Step 5 | `(cd koolade-exit-fixture && find . -path ./.git -prune -o -type f \( -path './planning/*' -o -path './.planner/*' \) -print0 \| sort -z \| xargs -0 sha256sum \| sha256sum)` → after-state digest EQUALS the pinned before-state digest ·· `git log -1 --pretty=%h` unchanged ·· transcript holds the verbatim `⚠ Turn rejected — nothing was written.` line (fatal) or the `dropped:` notice (soft) |
| (g) | Mid-turn cancel drops fragments cleanly | Step 6 | same `find … \| sort -z \| xargs -0 sha256sum \| sha256sum` digest post-cancel EQUALS the `hashes-C.txt` pin ·· `git log --pretty='%h\|%an\|%ae\|%s' -- planning .planner` unextended versus the pinned head |

---

## Retries (operator discipline — binds the operator of the demo, not the model)

1. Log every induction attempt (attempt number, verbatim prompt,
   observed response) in the transcript BEFORE escalating.
2. Strengthen by quoting the EXACT JSON fragments to embed
   (`{"id":"CLR-999","priority":"blocking"}`, `"CLR-999"`, and
   `"urgent"` respectively) as spelled in Step 5.
3. Escalate I1 → I2 → I3 in that order, ONE level per attempt.
4. After THREE failed inductions toward one outcome, REBUILD THE FIXTURE
   from scratch (fresh Step 0 into a fresh destination; retire the
   tainted one) and replay the affected steps.
5. Never hand-edit `planning/` or `.planner/`, never amend/cherry-pick/
   rewrite the fixture's history retroactively — zero-mutation proofs
   are only valid if the history remained pristine. Corrupted history
   voids the whole run: rebuild and replay.

---

## Known boundaries (why the demo is shaped this way)

- The walk is SINGLE-ACTOR and the fixture is TOY-scale BY DESIGN
  (every file trivially readable; D-20/D-21 moved real pilot projects
  and scale stress past the MVP edge).
- The collaboratively-staffed variant awaits the post-MVP
  collaboration channels (D-22); a future runbook section will extend,
  never replace, this one.
- §12.1 certification stands ON TOP OF, not instead of, §12.2: at sign-off
  the transcript records a concurrent-green NFR-8 suite invocation.
