---
<!-- koolade-artifact-id:v1 {"uid":"14ee508f-37a5-436e-a932-4e6ec4a5d87d","displayId":"F9-TASK-rewrite-the-product-index-h1-and-intro-to-the-canonical-name","title":"Rewrite the product index H1 and intro to the canonical name","parentUid":"006525e9-d85f-4b17-b05b-0dc1320d09d8"} -->

koolade-task: {"schemaVersion":1,"uid":"14ee508f-37a5-436e-a932-4e6ec4a5d87d","batchUid":"006525e9-d85f-4b17-b05b-0dc1320d09d8","repositoryId":"root","dependencyUids":[]}
---

# F9-TASK-rewrite-the-product-index-h1-and-intro-to-the-canonical-name — Rewrite the product index H1 and intro to the canonical name

Feature: Relabel product index to the canonical name (F9)

Status: Completed during the 2026-10-04 public-launch cleanup.

## Original problem this ticket solved and why

The index front page still titles the project Packet with bootstrap-era intro text, contradicting the canonical Kool.ad/e name adopted across the spec (CLR-001, F8); as sole editor this front door mislabels the product.

## Ticket goal — what changes when done

After this edit, the index H1 reads Kool.ad/e followed by em dash and Living Technical Specification, the old bootstrap invite is replaced by a concise manifest-order module orientation, and all other index content is unchanged.

## User story

As Alex Developer I want the front page titled Kool.ad/e with current orientation so the specification opens correctly instead of showing outdated Packet bootstrap text.

## Purpose

Apply the operator-approved two-part edit to .koolade-packet/planning/product/index.md: set the H1 to exactly 'Kool.ad/e — Living Technical Specification' and replace the bootstrap-era placeholder intro with a concise manifest-order orientation, leaving module order, manifest registration, and all other index content substantively untouched.

## Specification references

Source: [Approved specification](specification.md)

## Implementation context at task drafting

File .koolade-packet/planning/product/index.md holds the H1 Packet em dash Living Technical Specification plus placeholder intro inviting readers to describe the project in chat. A module manifest block sits below listing modules in a defined order. Only the H1 and intro paragraphs belong to this task.

Feature ID: F9
Repository: root

## Approved scope mapping

- Scope 1: Rewrite the H1 and intro paragraph of .koolade-packet/planning/product/index.md
- Success criterion 1: product/index.md H1 reads exactly "Kool.ad/e — Living Technical Specification"
- Success criterion 2: No bootstrap-era placeholder remains in the index intro; the replacement orientation matches the manifest-order module list
- Success criterion 5: Module order, manifest registration, and all other index content are unchanged in substance

## Dependencies

None. This task can start independently.

## Affected files and components

- .koolade-packet/planning/product/index.md - holds the H1, intro, and module manifest being corrected here

## Implementation steps

1. Open .koolade-packet/planning/product/index.md and read the full file.
2. Replace the H1 line with Kool.ad/e em dash Living Technical Specification preserving leading hash space syntax.
3. Locate the bootstrap intro paragraph beneath the H1 and rewrite it into one concise orientation naming the modules in manifest order.
4. Leave the module manifest block, ordering, registration, and every following section byte-for-byte otherwise identical.

## Acceptance criteria

- H1 equals exactly Kool.ad/e em dash Living Technical Specification.
- Intro contains no chat-describe-project placeholder and matches the manifest module order.
- Diff shows only the H1 line and intro paragraph changed.

## Test plan

1. Inspect the edited file visually confirming the new H1 and rewritten intro.
2. Run a diff of the file against HEAD expecting changes confined to the H1 and intro lines.

## Verification commands and expected evidence

1. git diff -- .koolade-packet/planning/product/index.md shows edits limited to the H1 and intro lines.

## Edge cases and failure handling

- If the intro spans multiple paragraphs confirm only the bootstrap portion is rewritten and no surrounding module content shifts.

## Constraints

- The H1 must read exactly "Kool.ad/e — Living Technical Specification"; the current hosting repository is `zbarno/kool.ade`
- Approval of F9 is already current for this contract; this turn makes no specification or open-item changes
- The change was applied directly by the operator during public-launch cleanup, since neither planner turns nor app-applied module updates can rewrite product:index
- Standing quality gates (fmt, test, clippy) apply before any resulting commit per the Decisions module

## Out of scope

- Module-body relabeling (completed in F8)
- Repository renaming; the public-launch decision keeps `zbarno/kool.ade`
- Any change to module order, manifest registration, or other index content

## Definition of done

- The checked-in snapshot carries the exact new H1 and replacement intro; app-refresh stickiness is verified in the F9 closure note.

## Instructions for the implementing model

Read this story and its linked dependencies before editing. Confirm the named existing files and interfaces; proposed files are explicitly labeled. Implement only this task's scope, preserving the constraints above. Run the verification commands and record actual results, changed files, and any remaining limitations. Do not mark complete on a build alone when acceptance criteria require runtime interaction. If an assumption contradicts the repository or approved specification, report the discrepancy rather than silently inventing a requirement.
