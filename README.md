# Kool.ad/e

---

## Experimental software — use on a test repository 

> **⚠️ Kool.ad/e isn’t fully mixed yet.**
>
> Kool.ad/e is under active development and has not been fully security- or reliability-vetted. **Do not use it on repositories, infrastructure, credentials, or projects you can’t afford to lose, corrupt, or expose.**
>
> For now, use disposable clones or test repositories, keep independent backups, review generated changes, and leave automatic publication disabled unless you knowingly accept the risk.
>
> The sandbox, independent task clones, human approval gates, and verification checks reduce risk. They are not a guarantee against surprising behavior.
>
> **Drink responsibly. Code experimentally.**

---

<div align="center">
  
# 🚨EXPECT BREAKING CHANGES AND INSTABILITY🚨

### Drink it and get S*** done!

**A native desktop planning and execution layer for agentic software development.**

Turn vague ideas into living specifications, actionable tasks, verified code, and significantly fewer
*"wait... why did the agent do that?"* moments.

![Kool.ad/e workspace with its redesigned header and blue task activity graphs](site/assets/board-preview.png)

**Rust · Linux · Git-native · Agent-powered · Zero grams of sugar**

</div>

> **Currently shipping:** an experimental Linux desktop app with Git-backed planning, independent task clones, concurrent workers, verification, and six coding CLI adapters. **Being built and queued:** separate multi-user planning repositories, Docker-only execution, dependable CLI discovery and role parity, secure remote workers, and interactive runtime QA. The roadmap below separates the two.



## What is Kool.ad/e?

Kool.ad/e is a desktop application for planning and building software with AI coding agents.

You bring it an idea:

> "I want users to be able to save searches."

Kool.ad/e helps turn that into:

**Idea → Clarification → Living Spec → Approved Plan → Task Stories → Implementation → Verification → Review/Publish**

The important bit is everything in the middle.

Most coding agents are very good at writing code once they understand what they're supposed to build. Unfortunately, humans are exceptionally talented at saying things like:

> "Just add saved searches. It should be pretty straightforward."

Kool.ad/e lives in the gap between those two realities.

It teases out ambiguity, maintains the specification, tracks unanswered questions, decomposes approved work, coordinates coding agents, verifies their output, and keeps you in control of what actually gets built.

Basically, project management for people who would rather be writing software than project-managing software.

---

## Meet Kool.ad/e Man.ager

The main agent you interact with is **Kool.ad/e Man.ager**.

No relation.

Kool.ad/e Man.ager acts as the planning and coordination layer between you, your repository, and the coding agents doing the work.

Instead of throwing an entire product conversation into one enormous chat session and hoping everyone remembers what happened 80,000 tokens ago, Man.ager maintains durable project knowledge in your repository.

It can:

- interview you about a feature or project
- identify questions, ambiguities, assumptions, and ownership gaps
- maintain the living product specification
- recommend implementation approaches
- generate ordered implementation tasks
- coordinate implementation workers
- verify completed work
- surface blockers when human judgment is actually required

It is less "AI pair programmer" and more "slightly obsessive technical project manager who happens to have access to coding agents."

And unlike certain completely unrelated mascots from your childhood, Kool.ad/e Man.ager generally prefers **full Git clones** to entering rooms through the wall.

---

## The board is the interface

Kool.ad/e doesn't revolve around one giant chat window.

The **Kanban board is the workspace**.

Questions, ambiguities, assumptions, planning work, implementation tasks, reviews, and blockers all live together as first-class items.

Each item gets its own focused conversation and context.

That means a discussion about database migrations doesn't quietly contaminate the context for a completely unrelated authentication task three days later.

Cards move through:

**To do → In progress → In review → Needs attention → Done**

When Kool.ad/e needs something from you, the question appears directly on the item that needs it.

Answer it there.

Continue drinking responsibly.

### Take a closer look

**Task details** bring the conversation, next action, and recorded activity together.

![Task details with a focused conversation, implementation actions, and activity graph](site/assets/task-details-preview.png)

**Workspace settings** group application preferences, coding tools, automation, and project controls.

![Workspace settings with grouped navigation and coding-tool configuration](site/assets/settings-preview.png)

Screenshots use an example workspace. Select an image to see it at full size.

---

## Living specifications, not chat archaeology

Chat history is useful.

Chat history is also a terrible product specification.

Kool.ad/e maintains a **Git-backed living specification** describing the product as it exists now.

Specifications capture things such as:

- product overview
- users and desired outcomes
- current capabilities
- architecture and constraints
- important decisions
- quality and acceptance expectations

Changes are planned separately, reviewed, approved, implemented, and eventually reconciled back into the product specification.

Git keeps the history.

The specification keeps the truth.

This prevents the traditional agentic-development archaeology expedition:

> "I know we decided this somewhere around message 847..."

---

## Planning that actually ends

Planning should produce something buildable, not an indefinitely expanding conversation.

For a feature or new project, Kool.ad/e works toward a clear contract:

1. Understand the goal.
2. Identify missing information.
3. Resolve important ambiguity.
4. Define scope and exclusions.
5. Establish constraints and acceptance criteria.
6. Produce an implementation-ready specification.
7. Ask you whether it should proceed.

Nothing starts building simply because the agent became excited about its own plan.

You approve the plan first.

A revolutionary governance model.

---

## From specification to task stories

Once a specification is approved, Kool.ad/e can generate ordered implementation stories.

Each story contains the context needed to perform that piece of work, including:

- the problem being solved
- intended outcome
- affected areas
- implementation guidance
- dependencies
- acceptance criteria
- tests
- verification commands

Tasks are generated as durable Markdown artifacts in the repository.

For example:

```text
.koolade-packet/planning/tasks/saved-searches/
├── README.md
├── specification.md
├── 001-persist-named-search-filters.md
└── 002-build-the-saved-search-picker.md
```

Generation is resumable. Completed stories are preserved if generation is interrupted, and user-edited stories are not silently overwritten.

Because "the AI regenerated my ticket and deleted the part I fixed" is not a feature.

---

## Let the agents loose. Carefully.

Kool.ad/e can move beyond planning and coordinate implementation.

Approved tasks are executed in **isolated full Git clones**, allowing multiple independent workers to operate concurrently without turning your working copy into a crime scene.

The worker pool is configurable from **1 to 8 concurrent tasks**, with **3** as the current default.

Dependencies are respected. Independent work can run concurrently, including planning or task generation while implementation workers continue. A task waiting for review or attention does not stop unrelated ready tasks from using the remaining worker capacity. Ready tasks that cannot start yet remain queued and the board reports when all worker slots are occupied.

Each task gets:

- its own task clone
- its own agent session
- implementation progress
- verification
- failure recovery
- preserved evidence
- independent cancellation

Failed work stays isolated.

Your main checkout stays yours.

---

## Human-gated autonomy

Kool.ad/e is designed to automate the tedious parts without quietly acquiring executive authority over your repository.

Planning, implementation, and publication are separate capabilities.

By default:

| Capability | Default |
| --- | --- |
| Planning assistance | ✅ |
| Automatic investigation | Configurable |
| Build approved work | ✅ |
| Publish verified changes | ❌ |
| Bypass failed verification because the agent seems confident | Absolutely not |

Generating tasks does **not** start implementation.

Approving a plan does **not** publish code.

Verified work can remain local for inspection until you decide what happens next.

When automatic publication is enabled, Kool.ad/e integrates work against the latest remote branch and reruns verification. The verified result stays local for your review until you choose **Share verified work**; sharing pushes a branch and opens a pull request.

No green checks, no share.

The robots have boundaries.

---

## Sandboxed agent execution

On Linux, autonomous repository access runs through **Bubblewrap**.

Planning and implementation sandboxes:

- scrub inherited environment variables
- hide host home and credential directories
- restrict repository access
- isolate network access
- prevent the agent from casually rummaging around the rest of your machine

Implementation happens in dedicated task clones rather than your active checkout.

Kool.ad/e also uses locks around task execution and publication so multiple processes cannot casually race each other toward increasingly creative Git history.

If the required sandbox cannot be established, autonomous execution fails closed.

The [filesystem boundary](docs/sandbox-filesystem.md) documents the runtime
allowlist, explicit project/toolchain mounts, and remaining security scope.

This describes **today's implementation**, not the target architecture. The [Docker-only execution migration](https://github.com/zbarno/kool.ade/issues/133) will replace Bubblewrap, preserve the existing authorization and Git safeguards, and prepare local and remote workers to execute under one policy. [Complete Bubblewrap removal](https://github.com/zbarno/kool.ade/issues/143) is an explicit cutover requirement, not a fallback option.

In technical terms: **the current bubble stays until the replacement passes its tests.**

---

## Verification is part of the job

Kool.ad/e doesn't consider "the agent said it works" to be a test strategy.

Implementation tasks carry explicit verification commands and acceptance criteria.

For Kool.ad/e itself, the current definition-of-done gates are:

```sh
cargo install cargo-nextest --version 0.9.148 --locked
cargo +1.98.1 fmt --all --check
cargo +1.98.1 nextest run --locked --all-targets --no-fail-fast
cargo +1.98.1 test --locked --doc
cargo +1.98.1 clippy --locked --all-targets -- -D warnings
```

Failed verification prevents publication.

Agents can attempt corrections and root-cause repairs, but they cannot simply explain why the failing test is probably unrelated and mark the task done anyway.

We've all met that developer.

---

## Agent activity you can actually see

Working items expose live activity so you can tell whether an agent is:

- thinking
- inspecting files
- running tools
- implementing
- verifying
- stuck
- contemplating the heat death of the universe while your local model processes a prompt

Kool.ad/e includes per-task activity views plus an aggregate activity graph across running work.

The graph represents observed agent activity, not fake precision about token throughput or productivity.

Sometimes a flat line just means the model is thinking.

Sometimes it means your 27B local model has wandered into the woods.

Both are valid possibilities.

---

## Built for local models too

Kool.ad/e assumes AI work may occasionally take longer than twelve seconds.

Planning turns currently allow up to **12 hours** by default and can be adjusted with:

```sh
KOOLADE_TURN_TIMEOUT_SECS=36000 koolade
```

Long-running local inference is treated as normal rather than evidence that civilization has ended.

**Pi** supports configured private/local HTTP OpenAI-compatible providers, including local models. The application also includes adapters for Codex, Claude Code, OpenCode, GitHub Copilot CLI, and Antigravity. Their provider-specific setup and execution capabilities are still being standardized.

The model is not embedded in Kool.ad/e. Agent/model communication happens through an external CLI harness; Kool.ad/e coordinates the work.

Which brings us to...

---

## Bring your own code

Kool.ad/e has an `AiHarness` abstraction separating the application from the coding-agent CLI underneath it.

**Already integrated:** Pi, Codex CLI, Claude Code, OpenCode, GitHub Copilot CLI, and Antigravity.

That does **not** yet mean every provider has identical capabilities, or that discovery never trips over a troublesome installation. [Crash-proof discovery](https://github.com/zbarno/kool.ade/issues/145) and [universal CLI role parity](https://github.com/zbarno/kool.ade/issues/146) are explicit prerequisites of the Docker migration.

**The direction:** any supported, compatible coding CLI can handle planning, task generation, implementation, independent QA, documentation, and Man.ager work through the same application-owned, policy-enforced tools. No "Pi gets the good tools and everybody else gets a security error" arrangement.

The goal is to manage the development process without requiring you to marry one agent ecosystem.

---

## Current status

Kool.ad/e is **v0.1.0 and under active development**.

More importantly, it is already being used to build itself.

That's either dogfooding or the beginning of an extremely niche science-fiction plot.

Current capabilities include:

| Area | Status |
| --- | --- |
| Native desktop UI | ✅ |
| Git-backed living specifications | ✅ |
| Kanban planning workflow | ✅ |
| Per-task conversations | ✅ |
| Questions / ambiguities / assumptions / ownership | ✅ |
| Architectural decision records | ✅ |
| Plan comparisons | ✅ |
| Task-story generation | ✅ |
| Resumable task generation | ✅ |
| Concurrent implementation workers | ✅ |
| Isolated task clones | ✅ |
| Verification and recovery | ✅ |
| Optional automatic publication | ✅ |
| Pull request state tracking | ✅ |
| Multi-repository projects | ✅ |
| Bubblewrap sandboxing | ✅ |
| Pi CLI | ✅ |
| Codex CLI | ✅ |
| Claude Code CLI | ✅ |
| Antigravity CLI | ✅ |
| OpenCode CLI | ✅ |
| GitHub Copilot CLI | ✅ |
| Linux x86_64 | ✅ Current host |
| External managed planning repository | ✅ One planning repository per workspace; shared Git sync and per-record conflicts |
| Consistent CLI role support + resilient discovery | 🛠️ Queued migration |
| Docker-only local execution | 🛠️ Queued migration |
| Remote Docker workers | 🛠️ Queued migration |
| Interactive UI / runtime QA | 🥤 Proposed direction |
| macOS | 🥤 Planned through Docker Desktop |
| Windows | 🥤 Planned through Docker Desktop |
| Additional agent CLIs | 🥤 Very much planned |
| Telepathic requirements gathering | ❌ Pending model improvement |

### Current constraints

Right now Kool.ad/e is deliberately opinionated:

- **Linux x86_64 only**
- shared planning is coordinated through an explicitly configured Git remote and manual sync; it does not provide live notifications or per-field access control
- supported agent CLIs use an application-managed Bubblewrap boundary for repository commands
- autonomous repository access currently requires **Bubblewrap**; **Docker is planned, not required for this release**
- Pi's provider bridge currently targets **private/local HTTP OpenAI-compatible endpoints**
- a CLI adapter being integrated does not guarantee every role works with every version yet
- authenticated remote execution and interactive runtime QA are not yet shipped

If you need something polished, cross-platform, and enterprise-ready today, this is probably early for you.

If you enjoy software that is already useful while still being actively invented, welcome aboard.

---

## Roadmap: from one machine to a whole crew

Kool.ad/e began as a Linux desktop app with a planning board and isolated local coding agents. The next version of the idea is a **Git-backed, multi-operator, Docker-executed work manager**, with workers that can eventually run wherever you trust them.

**The distinction matters:** checked-in code and completed issues are not the same thing as a fully tested, released feature. The steps below describe the present system, active cutover work, and queued architecture without pretending the whole drink has been mixed.

```mermaid
flowchart LR
    A["01 · TODAY<br/>Linux desktop<br/>Git-backed planning<br/>Task clones + 6 CLI adapters"] --> B["02 · IN FLIGHT<br/>Standalone planning Git repos<br/>Multi-user sync + task claims<br/>Safe branch/PR coordination"]
    B --> C["03 · QUEUED<br/>Docker-only tool execution<br/>CLI parity + reliable discovery<br/>Remote SSH workers"]
    C --> D["04 · ON THE HORIZON<br/>Interactive visual / functional QA<br/>macOS + Windows hosts<br/>Smarter context + notifications"]
```

### 01. In the glass today

Kanban planning, task-specific conversations, living specifications, six CLI integrations, independent Git clones, concurrent local execution, verification, and human-controlled publication. **Linux x86_64 with Bubblewrap remains the current execution environment.** Useful, but experimental.

### 02. Shared planning across multiple operators

Each workspace has one external **planning repository**, independent of its zero or more code repositories. Operators join the same workspace by connecting to that repository; local code checkout mappings remain operator-specific. Shared planning uses normalized per-record files, explicit Git sync, task claims, branch/PR linkage, and conflict recovery. Legacy embedded artifacts remain migration and recovery inputs; migration preserves the originals.

Planning-only workspaces are supported. Code branches belong to their selected code repository and do not change planning authority. See [the planning repository contract](docs/planning-repository-v1.md) and [cutover and recovery notes](docs/releases/2026-10-planning-repository.md) for sync, recovery, security, and limitations.

### 03. Next major architectural upgrade: Docker everywhere work executes

Move all repository tooling to a single Docker-backed execution policy; remove Bubblewrap after the replacement is verified. Give **all six CLIs** the same approved capabilities for every role, fix fragile CLI scanning and host-code-mode failures, provision dependencies through controlled brokers, then add authenticated **remote Linux Docker workers over SSH**. Workspaces and evidence travel via verified transfers; provider secrets and Git publication authority do not.

The initial shared request/policy/event contract is documented in [Execution architecture v1](docs/execution-architecture-v1.md). The Docker migration series is tracked in [#133–#146](https://github.com/zbarno/kool.ade/issues/133), with dedicated [remote worker transport](https://github.com/zbarno/kool.ade/issues/139) and [verified workspace synchronization](https://github.com/zbarno/kool.ade/issues/140) issues.

**Docker is the destination, not a current installation requirement.** Remote workers are queued, not shipping.

### 04. After the foundation: agents that check their own homework

Autonomous runtime QA should launch a finished app, interact with the UI, inspect visuals and behavior, capture evidence, and send defects back through controlled repair loops. Browser automation and containerized Linux UI tests come first; native platform-specific testing will need additional workers and drivers.

Also in the mix: **macOS and Windows controller support**, better completion/blocker alerts, improved context selection and token economics, smarter model and harness routing, and more coding CLIs.

No telepathic requirements gathering. We're keeping that one in research.

---
## Install

### Requirements

For the current release you'll need:

- Linux x86_64
- Git
- Rust / Cargo
- Bubblewrap for **current** autonomous planning and implementation (the Docker cutover has not shipped yet)
- at least one supported coding CLI: Pi, Codex, Claude Code, OpenCode, GitHub Copilot CLI, or Antigravity, configured for its supported work modes
- if using **Pi with a local model**, a private/local HTTP OpenAI-compatible provider configured for Pi

Clone the repository:

```sh
git clone https://github.com/zbarno/kool.ade.git
cd kool.ade
```

Install Kool.ad/e under `~/.local`:

```sh
./scripts/install.sh
```

Or install somewhere else:

```sh
./scripts/install.sh --prefix /absolute/path
```

Then launch Kool.ad/e and configure your preferred coding CLI from **Workspace → Settings → Coding Tools**. The upcoming Docker and universal-role work will simplify setup further.

### Run from source

Once dependencies have been downloaded:

```sh
cargo +1.98.1 run --offline
```

Kool.ad/e is currently developed with Rust toolchain **1.98.1**.

---

## Where does everything live?

Kool.ad/e intentionally keeps durable project knowledge close to the code.

### In the repository

```text
.koolade-packet/
├── config/
├── planning/
│   ├── product/
│   ├── changes/
│   ├── tasks/
│   └── ...
└── state/
```

This is the **legacy embedded planning layout currently supported by existing workspaces**. The ongoing planning-repository cutover moves shared planning truth to an independent, per-workspace Git repository with one file per mutable record, leaving code branches and checkouts independent. See [the planning repository contract](docs/planning-repository-v1.md) and [cutover issue #120](https://github.com/zbarno/kool.ade/issues/120).

### Outside the repository

Operator-specific state such as conversation history and resumable local checkpoints lives under:

```text
~/.koolade-packet/projects/<project>/
```

or the location configured through `KOOLADE_HOME`.

Implementation evidence is kept separately from the durable specification so your product documentation does not slowly become a landfill of terminal output.

For the full layout, see [`docs/artifact-layout.md`](docs/artifact-layout.md).

---

## Architecture

Kool.ad/e is a native Rust application using:

- **Rust 2024**
- **egui / eframe** for the desktop UI
- **Git** as the durable history substrate
- **Markdown** for human-readable planning artifacts
- external coding-agent CLIs through the `AiHarness` boundary
- **Bubblewrap** for current Linux execution isolation

**Target architecture (queued, not yet shipped):** the same Rust planning and harness layers backed by a Docker-only local/remote execution service, authenticated SSH workers, bounded dependency and evidence flows, and independent shared planning Git repositories. Bubblewrap will be removed after the Docker security and migration gates pass.

There is no embedded LLM.

Kool.ad/e manages the work.

Your chosen agent harness manages the model.

Git remembers what everybody did.

A surprisingly healthy separation of responsibilities.

---

## Development

Run the application:

```sh
cargo +1.98.1 run --offline
```

Run the quality gates:

```sh
cargo install cargo-nextest --version 0.9.148 --locked
cargo +1.98.1 fmt --all --check
cargo +1.98.1 nextest run --locked --all-targets --no-fail-fast
cargo +1.98.1 test --locked --doc
cargo +1.98.1 clippy --locked --all-targets -- -D warnings
```

The project intentionally keeps modules small and favors explicit, testable state transitions over agent-generated mystery meat.

If you're changing planning behavior, artifact structure, sandboxing, implementation orchestration, or publication semantics, expect tests.

Lots of tests.

The agent has feelings about this.

They are ignored.

---

## Why does this exist?

Coding agents keep getting better at implementation.

That shifts the bottleneck.

As coding gets faster, unclear requirements can also get you to the wrong thing faster.

The hard part increasingly becomes:

- deciding what should be built
- explaining intent clearly
- identifying ambiguity before implementation
- maintaining product knowledge over time
- decomposing work sensibly
- keeping parallel agents coordinated
- verifying what they actually produced
- deciding when humans need to intervene

Kool.ad/e is an experiment in treating **that layer** as a first-class development tool.

Not just:

> "AI, write this function."

More like:

> "Help me continuously turn product intent into verified software without losing the plot."

That's the idea.

---

## Contributing

Kool.ad/e is being developed in public because software about collaborative agentic development seems like a particularly silly thing to build entirely behind closed doors.

Issues, ideas, bug reports, design discussions, and pull requests are welcome.

If you find something weird, please include enough information to reproduce it.

If an agent did something weird, include that too.

Especially if it tried to achieve project goals through drywall.

---

## License

Kool.ad/e is licensed under the **MIT License**. See [`LICENSE`](LICENSE).

---

## Extremely important legal beverage information

Kool.ad/e is an independent software project.

It is not affiliated with, sponsored by, endorsed by, or otherwise connected to any beverage company, powdered drink mix, animated pitcher, wall-destroying mascot, childhood sugar rush, or suspiciously red liquid you may remember from another era.

Any resemblance is completely coincidental.

Obviously.

---

<div align="center">

### Kool.ad/e

**Drink it and get S*** done!**

Planning software so the coding agents know what the hell you actually meant.

</div>
