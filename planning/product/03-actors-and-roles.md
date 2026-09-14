## 3. Actors and Roles

- **Operator:** the human at the desktop app. Identity is **derived from the git user of the connected working tree** at connection and after settings saves: `git config user.name` first, `user.email` as fallback, then the optional `## Current User` block in `.planner/config.md`, and finally a neutral **(guest)** if all sources are empty (D-14). The settings dialog keeps the identity fields, re-purposed: they echo the derived identity and serve only as an override when git yields nothing.
- **Roles and ownership (D-14).** Roles correspond to the categories of the planner configuration, each owned by a person or a group:
  - **Explicit person ownership is sole-owned.** If a role is explicitly defined for a particular user, that user is the only owner of that category of items: only they are asked its questions; no one else — including the chair — inherits or redirects them. Their items remain visible in everyone's panel.
  - **Group ownership is shared.** A role defined for a group is co-held by its members (existing behavior, preserved).
  - **Seat inheritance is the default.** Any role with *no* explicit owner is assumed by the currently logged-in (git-identified) operator by default. Nothing orphans for want of a paper owner; Ownership items (FR-7) then serve as formal nominations rather than rescue tickets.
- **Stakeholders:** holders of open items, organized by category (Contract §8).
- **Planning agent** (this voice): interviewer and investigator; read-only over the repository except for the planning artifacts it owns.

Current ownership state (v1.2, D-25 — every lane is named):

| Category | Configured owner | Effective routing |
| --- | --- | --- |
| General | — (structural) | Broadcast: every seated operator |
| Product | Zachary Barno (sole) | Chair as explicit sole owner (law rule c) |
| Development | Zachary Barno (sole) | Chair as explicit sole owner (law rule c) |
| QA | Zachary Barno (sole) | Chair as explicit sole owner (law rule c) |
| InfoSec | Zachary Barno (sole) | Chair as explicit sole owner (law rule c) |
| UX | Zachary Barno (sole) | Chair as explicit sole owner (law rule c) |
| Operations | Zachary Barno (sole) | Chair as explicit sole owner (law rule c) |

Chair's seat (D-23, completed by D-25): the operator elected to sit under the connected repository's git identity — **Zachary Barno** ⟨zbarno@gmail.com⟩ — and has now formally nominated THEMSELVES as sole owner of every lane (settings save `fb54229`). Three properties follow: (1) **operational parity for the sole operator** — every question still reaches the chair; only the legal basis shifts, from seat inheritance (law rule d) to explicit sole ownership (rule c); (2) **strictness travels** — if a second seat is ever seated, these six lanes are **vetoed** off it (claimed by other holders, outraking direct address); a sole nomination is an exclusion, chosen knowingly as the strongest form; (3) **wake conditions discharged** — v0.7's two-stage gate (session seated under the git identity AND the F-8 delta shipped) is closed: FR-13 and the D-14 routing delta are verified landed on master (tickets 001/002, v1.2 pass), and the running build already renders the per-seat lane digest (this turn's operator context lists Sole-owned lanes: Product, Development, QA, InfoSec, UX, Operations) — rule (c) recognition is live, not owed. Consequently FR-7 has no remaining gap to synthesize in this repository, and the nomination tickets CLR-008 through CLR-013 that once carried the ask are resolved by D-25; explicit renomination later (another person or a group) simply edits the same grid — nothing structural changes.
