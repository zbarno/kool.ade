# Kool.ad/e redesign and rebrand

**Status:** Implementation specification

**Scope:** Presentation and user-visible branding of the Kool.ad/e desktop app

**Reference:** [target board composition](../assets/brand/board-reference.svg), [supplied logo](../assets/brand/kool-ad-e-logo-source.png)

## Goal

Rebrand the app as **Kool.ad/e**. Keep its specification, board, conversations, agent work, Git persistence, and human approval behavior intact. The visual direction is a dark engineering tool with Tropical Punch red atmosphere and small, crisp Kool Blue interactions. The UI should remain readable and practical during long work sessions.

The supplied logo is the source of truth for the wordmark and exact tagline. The board reference SVG is an implementation guide assembled from the approved layout and palette, not a pixel-perfect screenshot of an existing build. Obsolete before/after screenshots are not part of the public asset set.

## Brand identity

- Product name: `Kool.ad/e`, with that exact casing and punctuation.
- Tagline: `Drink it and get S*** done!`, with **S followed by three asterisks**, as shown in the supplied logo. Keep the tagline white.
- Wordmark: white letters; the `.` and `/` are Kool Blue. Preserve the supplied chunky letterforms and dark blue outline. Do not recreate the display lettering with a system font.
- Use the full logo at the upper left of the desktop header. Per the 2026-09-29 request for a larger, flashier identity, display the full lockup at 210 px wide (about 106 px high) in the desktop banner, without distortion. Hide the tagline in narrow layouts rather than making it unreadably small.
- The supplied logo PNG is 282×142 px and has a baked-in red background. Keep it as a reference asset. For production use on a neutral header, obtain or prepare a faithful transparent, higher-resolution export before shipping. Do not silently remove its background or redraw its words inaccurately.
- Use the supplied tagline in brand lockups and splash/empty states. Normal app status language remains concise and technical.

## Design tokens

The accompanying [CSS token sheet](../assets/brand/tokens.css) is the exact palette reference. Map these values to the existing Rust UI theme rather than introducing a CSS runtime solely for branding.

| Role | Token | Hex | Usage |
| --- | --- | --- | --- |
| Canvas | `--surface-canvas` | `#080B0E` | App background |
| Column | `--surface-column` | `#0D1217` | Board lanes |
| Raised | `--surface-raised` | `#131A20` | Header, panels |
| Card | `--surface-card` | `#182129` | Task cards |
| Card hover | `--surface-card-hover` | `#1D2831` | Hover/selection |
| Border | `--border-default` | `#2D3942` | Neutral 1 px edge |
| Strong border | `--border-strong` | `#3C4A54` | Hover/focus structure |
| Primary text | `--text-primary` | `#F4F5F2` | Titles and body |
| Secondary text | `--text-secondary` | `#A7ADB4` | Supporting text |
| Muted text | `--text-muted` | `#78838D` | IDs and metadata |
| Punch red | `--brand-punch` | Live activity graph, selected board accent |
| Punch bright | `--brand-punch-bright` | `#F3374D` | Small bright highlights |
| Kool Blue | `--brand-blue` | `#009FE8` | Primary action and active work |
| Blue hover | `--brand-blue-bright` | `#22B8FF` | Hover/progress |
| Attention | `--state-attention` | `#FFC247` | Human action required |
| Success | `--state-success` | `#45D483` | Done |
| Failure | `--state-failure` | `#FF5364` | Actual error/destruction |

Use deep crimson (`#4A0710`, `#680A15`, `#8E0D1B`, `#B71325`) only as controlled header ambience or small decorative accents. The canvas and cards remain neutral. Red should read as fruit punch or ruby, never magenta. Blue is for primary interaction and visible agent activity. Semantic colors have their own meanings.

Task types: Task `#C9D1D9`, Question `#B66CFF`, Ambiguity `#FFC247`, Assumption `#22B8FF`, Ownership `#F06A9D`. Lane status: To do neutral, In progress blue, In review purple, Needs attention amber, Done green. Keep labels/icons alongside colors.

## Desktop composition

1. **Compact red banner:** full logo at left; quiet repository/product context near center; branch, saved-to-git state, and Workspace at right. Per the 2026-09-29 visual correction, use an edge-to-edge deep Punch Red background, including the panel margins, with the supplied [transparent Tropical Punch splash](../assets/brand/tropical-punch-splash.png) visibly enlarged behind the logo. Crop and clip the splash at the banner boundary, preserve its aspect ratio, and keep it behind text and controls. Do not paint a separate rectangular background around the logo. Apply the same treatment at compact widths. It is decorative art and must not be used as activity data.
2. **Global activity graph:** retain the existing real-time graph and its underlying activity data. Restyle its line in Punch Red, with restrained contrast against the dark header. Its points, timing, peaks, idle periods, and update cadence must continue to represent actual activity. Do not replace it with a repeating SVG, arbitrary oscillation, decorative animation, or a fabricated waveform. Keep the current graph's meaning and interaction behavior. The line shown in the board reference is illustrative sample data only.
3. **Primary row:** `Specification` and `Board` aligned left, `+ New Task` on the far right of the same row. The selected tab uses a deep red fill or bright red edge. New Task uses Kool Blue fill, white text, and a blue hover treatment.
4. **Legend:** per the latest space-saving direction, place Task / Question / Ambiguity / Assumption / Ownership in the navigation row between Board and + New Task, centered within that remaining space. Remove the separate legend and instruction rows. Use a Types menu when the available width is limited; the Board tooltip also exposes the legend at the narrowest sizes.
5. **Board:** preserve five lanes and their order: To do, In progress, In review, Needs attention, Done. Differentiate canvas, columns, and cards with surface contrast and thin borders, not broad colored lane fills.

Use a 142 px desktop banner to accommodate the enlarged logo and a separate dark activity strip; keep the compact banner at 72 px. Use brighter splash art, strong selected-tab edges, colored lane dividers, and steady semantic card glows for the requested flashier treatment. Preserve readable controls and equal lane widths.

## Cards and states

Make the human title the strongest text. Use this order: title, type/state, short description, criteria/progress, activity, subdued machine ID. A long ID must truncate or wrap harmlessly without becoming a second title. Target title 15–16 px semibold, lane title 14 px semibold, body 13–14 px, metadata 11–12 px, tag 10–11 px. Use a neutral UI sans such as Inter, Geist, or system UI; reserve mono for IDs, paths, refs, hashes, timestamps, and logs.

Use 8 px corners for cards and lanes, 7 px for buttons/inputs, and pill radii only for small tags. Prefer 1 px borders and surface contrast to shadows. The card base is `#182129` with a `#2D3942` border; hover is `#1D2831` with a stronger edge.

- **Working:** subtle Kool Blue border/glow and a blue version of the card's existing real-time activity graph, where one exists. Preserve the card graph's data and time scale. The card itself does not pulse. Show a clear status such as `Kool.ad/e is working`.
- **Needs attention:** amber edge and explicit question/action, plus the person or group that must respond when known. An answerable item should show recommended options and always allow free text through its existing interaction. Reserve failure red for an actual error.
- **Done:** green signal, without filling the whole card green.
- **Empty lanes:** restrained line art and short useful copy. Avoid decorative clutter.

Keep type badges compact and colored, on neutral card backgrounds. Do not depend on the legend alone for identification.

## Motion, iconography, and voice

- Motion communicates an operation: a brief `fizz` on start and a small `pop` on state change. The activity graphs update only as new measurements arrive; do not animate invented activity. Aim for 150–300 ms UI transitions and respect reduced-motion settings without suppressing real data updates.
- Use one consistent monochrome line-icon style. Color the state indicator, not every icon.
- Keep operational copy clear: `Implementation complete`, `Verification failed`, `Waiting to merge`, `Needs attention`. Brand copy such as `Ready to mix` or `Needs an ingredient` may appear sparingly as secondary text. Do not rename the five Kanban lanes.
- The slash can recur occasionally as a brand separator (`PLAN / BUILD / VERIFY`), not as a replacement for standard labels.

## Scope and migration

Replace primary user-visible `Kool.ad/e` text, window title, and working status with `Kool.ad/e`. Internal crate names, namespaces, stored data, and artifact paths should remain stable unless independently required. In particular, `.koolade-packet/` remains the live shared project-artifact root under the repository's `AGENTS.md` rule. Do not turn a visual rebrand into a data migration.

Preserve task transitions, board filtering, specification behavior, acceptance criteria, ownership, task conversations, agent execution, Git storage, and human gates. Provide focus states and readable contrast. Status, type, errors, and completion must be understandable without color alone.

## Implementation order

1. Theme tokens and neutral dark surfaces.
2. Faithful logo export and compact header.
3. Navigation row, blue New Task, centered legend.
4. Board/card hierarchy and semantic signals.
5. Restyling of the existing global and card activity graphs, plus empty states.
6. Controlled header ambience and reduced-motion polish.

## Acceptance criteria

- Primary UI branding, title, and working state say `Kool.ad/e`, with white logo letters, blue `.` and `/`, and the exact white tagline `Drink it and get S*** done!` where the full lockup appears.
- The app has neutral near-black surfaces, Punch Red ambient accents rather than pink, Kool Blue primary actions and active work, amber human attention, green Done, and locally scoped failure red.
- `+ New Task` is right aligned on the Specification/Board row; the task-type legend sits between Board and New Task.
- Human titles dominate IDs; existing card and global activity graphs use blue and red line styling respectively while preserving their real-time data and behavior.
- The five lanes, all existing work flows, human gates, and `.koolade-packet/` data continue unchanged.
- Keyboard focus, status labels, contrast, responsive header behavior, and reduced-motion behavior remain usable.
- Decorative elements never cover controls, criteria, or task content.

## Asset notes

- `kool-ad-e-logo-source.png`: supplied wordmark and tagline, including its red background. Reference only until a faithful transparent high-resolution export exists.
- `board-reference.svg`: code-native target layout reference. Its simple typographic logo is schematic; implement with the approved art, not the SVG text rendering.
- `board-reference-preview.png`: rendered copy of the layout reference for quick viewing. Its graph paths are illustrative sample traces only.
- `tropical-punch-splash.png`: transparent red liquid splash for restrained header use; no logo or text is baked in.
- `app-icon.png`: dedicated square app/window icon using the punched-through red brick wall and Kool Blue `./` mark.
- `mark.svg`: scalable launcher/favicon version of the punched-through red brick `./` app mark. It is not a replacement for the full wordmark.
- `tokens.css`: portable palette reference to translate into the Rust theme.

## Task details polish — 2026-09-29

Use a concise Task details modal heading with one prominent human title below it, a compact task ID with full-path hover, a Kool Blue modal edge and Punch Red divider. Keep the modal identity keyed to the task path. Current state and next action have separate accented panels; failed independent checks use failure red. Worker output is expandable, while the measured activity graph and a two-line preview remain visible in a compact activity card. Preserve all existing approval, reply, resume, stop, copy, report, and navigation behavior. Reduce spacing and title size at narrow widths so the next action stays visible.
