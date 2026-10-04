## Overview

Kool.ad/e (crate `koolade`, v0.1.0, MIT) is a native desktop application for planning software projects through conversation with a local AI agent. It keeps a Git-backed living specification, a Kanban task board, and a gated workflow that turns approved plans into verified, optionally published code changes.

Key facts:

- **Identity.** The canonical project name is **Kool.ad/e**; the display name comes from `PRODUCT_NAME` in `src/lib.rs`. The hosting repository is [`zbarno/kool.ade`](https://github.com/zbarno/kool.ade) (`.koolade-packet/config/repositories.json`). Older planning records called the repository Packet and proposed a later rename; the public-launch plan superseded that proposal and keeps the current repository.
- **Positioning.** "Kool.ad/e teases out ambiguity and drives clear focused specifications" (`AGENTS.md`). The board is the primary workspace; the specification is the durable record of the product.
- **Dogfooding.** Kool.ad/e keeps its own living specification, planning history, and workflow state in `.koolade-packet/`, following `docs/artifact-layout.md`.
- **Platform.** Linux x86_64 desktop; autonomous repository access and implementation additionally require Bubblewrap (`README.md`, Host execution capabilities). Collaborative planning channels are deferred (see Users and Outcomes).

Maturity: a working system with extensive automated regression evidence; see Current Capabilities and Quality and Acceptance. Formal goals beyond the current workflow are not stated in the repository.
