## Decisions

The operator confirmed (CLR-002, F8) that the previous planning documentation was lost, including the formal decision records that belonged under `.koolade-packet/planning/decisions/`. Recreation is now directed: the implementation codebase is the primary evidence and the surviving maintainer documents are corroborating remnants. Where behavior is enforced, the code is authoritative; any disagreement between a remnant document and the code is surfaced as a board item (F8 R2), never absorbed silently. Individual ADR files are minted by Kool.ad/e only for new durable decisions approved through planning (records live at the canonical decisions root per `src/artifacts/layout.rs`; minting is restricted to approved Review decisions per `src/artifacts/koolade/decision.rs`), so none of the five lost-era IDs qualifies for that mechanism — this register is their durable reconstructed record.

**Reconstructed decision IDs** (rebuilt from the codebase per CLR-002)

| ID | Decision | Primary evidence (code) | Corroborating document | Status |
| --- | --- | --- | --- | --- |
| D-14 | Question-routing law per user: an item is poseable via broadcast (General), direct address, owned lane (sole personal or group-member holder), or seat inheritance for unowned lanes; a lane claimed by other holders is never poseable to the current user (veto outranks direct address); enforced at validation time, so a violating choice rejects the entire turn | `src/core/routing.rs` (the four rules, the veto, `Eligibility`/`LaneClaim`); enforcement in `src/core/validation.rs` | `docs/exit-demo-runbook.md` | Reconstructed from code |
| D-15 | In-app "Set up the pi harness" guide and live setup check; unmet setup blockers are recomputed on connect/retry and surface as derived board items | `src/harness/pi_harness.rs` (sole Pi CLI backend); `src/app/setup_attention.rs` (setup blockers per connect/retry); `src/harness/runtime_capabilities.rs` (capability detection) | `docs/exit-demo-runbook.md` (gate #4: Settings → "Set up the pi harness"; discovery order `KOOLADE_PI_BIN` → PATH → common install locations) | Reconstructed from code |
| D-22 | Single-operator MVP: collaboration channels deliberately parked; one chair seat per project and a single-writer invariant over planning artifacts | Single-seat construction: one git-derived `CurrentUser` drives routing while guests are not seated operators (`src/core/routing.rs`); no peer-collaboration transport exists in the source tree | `docs/exit-demo-runbook.md` ("D-22 parked the MVP walk around the absent collaboration channels: one chair seat, one app instance (single-writer invariant, NFR-3)") | Reconstructed from code |
| D-23 | The chair seat is derived from the connected repository's Git identity rather than entered as a Kool.ad/e account | Connect anchors on a git working tree and captures a live `gitops::snapshot` (`src/app/welcome/connect.rs` → `src/core/gitops.rs`) | `docs/exit-demo-runbook.md` (Step 0 uses the synthetic fixture identity Alex Developer); individual operator identity is not product truth | Reconstructed from code |
| FR-13 | Git-derived identity plumbed through connect/resync so seating, routing, and commit attribution share one identity | Snapshot at connect (`src/app/welcome/connect.rs`); the same seat feeds the D-14 routing law (`src/core/routing.rs`) and planner checkpoint attribution | `docs/exit-demo-runbook.md` (prerequisite note: "git-derived identity (FR-13) plumbed through connect/resync") | Reconstructed from code |

**Standing policy decisions**

| Decision | Basis | Status |
| --- | --- | --- |
| `.koolade-packet/` is the only live shared artifact root; legacy roots are migration inputs or history only | `AGENTS.md`; `docs/artifact-layout.md` | Standing |
| Plan recommendations are advisory; only explicit operator adoption selects a plan and unlocks approval | `AGENTS.md`; `README.md` | Standing |
| ADRs are created only for durable, consequential user-approved choices; routine choices and task completion do not create ADRs | `README.md`; `docs/planner-policy.md` | Standing |
| Six required specification concepts with concise modules, stable IDs, and explicit supersession of changed decisions | `docs/planner-policy.md`; product `manifest.json` | Standing |
| All three quality gates (fmt, test, clippy on Rust 1.98.1) must pass before a task is complete or committed | `AGENTS.md` | Standing |
| Canonical project name is `Kool.ad/e`; the existing hosting repository is `zbarno/kool.ade` and remains the launch repository | Public-launch cleanup plan; `.koolade-packet/config/repositories.json` | Adopted |
| Brand tagline is `Drink it and get S*** done!` (S + three asterisks), used in brand lockups and splash/empty states; `docs/kool-ad-e-redesign.md` is the governing branding contract | Operator ruling in CLR-004 (F8); `docs/kool-ad-e-redesign.md` | Adopted |
| Source-comment specification citations point at the living modularized product specification (`.koolade-packet/planning/product/`); stale `SPECIFICATION.md` section-number cites are re-pointed by concern, and the lost numbering is not resurrected | Operator ruling in CLR-004 (F8); `src/lib.rs` header precedent | Adopted |

**Supersession note**: the "Adopted, referenced only" treatment of D-14, D-15, D-22, D-23, and FR-13 is superseded by the reconstructions above per the operator direction recorded in CLR-002 (F8). Any future adoption or reversal amends this table explicitly; nothing else previously recorded here is superseded.
