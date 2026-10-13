# Kool.ad/e architecture atlas

> **18 views · 36 Mermaid diagrams**. Current means present in the October 13, 2026 codebase at the architecture level; Future means target design from the linked GitHub issues. Diagrams deliberately simplify detailed failure and security cases. The source issues and code remain authoritative.

[Explore the interactive gallery](https://kool.ad/diagrams) · [Marketing architecture tour](https://kool.ad/architecture)

## 01. System context

Target specification: [GitHub issue #172](https://github.com/zbarno/kool.ade/issues/172).

### Current architecture (simplified)

```mermaid
flowchart LR
    c0["User"]
    c1["Kool.ad/e desktop"]
    c2["Planning Git repository"]
    c3["Code repositories"]
    c4["Coding CLIs"]
    c0 --> c1
    c1 --> c2
    c2 --> c3
    c3 --> c4
```

### Future architecture (target)

```mermaid
flowchart LR
    f0["User"]
    f1["Kool.ad/e Man.ager"]
    f2["Shared planning remote"]
    f3["Docker execution targets"]
    f4["External providers"]
    f0 --> f1
    f1 --> f2
    f2 --> f3
    f3 --> f4
```

## 02. Controller components

Target specification: [GitHub issue #157](https://github.com/zbarno/kool.ade/issues/157).

### Current architecture (simplified)

```mermaid
flowchart LR
    c0["Board + Task Details"]
    c1["Planning core"]
    c2["Git controller"]
    c3["CLI adapters"]
    c4["MCP security bridge"]
    c0 --> c1
    c1 --> c2
    c2 --> c3
    c3 --> c4
```

### Future architecture (target)

```mermaid
flowchart LR
    f0["Board + Task Details"]
    f1["Event-driven lifecycle"]
    f2["Policy dispatcher"]
    f3["Docker backend"]
    f4["QA evidence pipeline"]
    f0 --> f1
    f1 --> f2
    f2 --> f3
    f3 --> f4
```

## 03. Agent roles and orchestration

Target specification: [GitHub issue #158](https://github.com/zbarno/kool.ade/issues/158).

### Current architecture (simplified)

```mermaid
flowchart LR
    c0["Manager"]
    c1["Planner"]
    c2["Coding harness"]
    c3["Verification"]
    c4["User gate"]
    c0 --> c1
    c1 --> c2
    c2 --> c3
    c3 --> c4
```

### Future architecture (target)

```mermaid
flowchart LR
    f0["Man.ager"]
    f1["Planner"]
    f2["Independent plan reviewer"]
    f3["Decomposer"]
    f4["Implementer"]
    f5["Code reviewer"]
    f6["Runtime QA"]
    f0 --> f1
    f1 --> f2
    f2 --> f3
    f3 --> f4
    f4 --> f5
    f5 --> f6
```

## 04. Feature and task lifecycle

Target specification: [GitHub issue #157](https://github.com/zbarno/kool.ade/issues/157).

### Current architecture (simplified)

```mermaid
flowchart LR
    c0["Idea"]
    c1["Planning conversation"]
    c2["Task documents"]
    c3["Implementation"]
    c4["Verification"]
    c5["PR decision"]
    c0 --> c1
    c1 --> c2
    c2 --> c3
    c3 --> c4
    c4 --> c5
```

### Future architecture (target)

```mermaid
flowchart LR
    f0["Idea"]
    f1["Async clarification"]
    f2["Plan approval"]
    f3["Independent review"]
    f4["Task DAG"]
    f5["Implementation"]
    f6["Independent QA"]
    f7["PR"]
    f0 --> f1
    f1 --> f2
    f2 --> f3
    f3 --> f4
    f4 --> f5
    f5 --> f6
    f6 --> f7
```

## 05. Planning ambiguity and approval

Target specification: [GitHub issue #159](https://github.com/zbarno/kool.ade/issues/159).

### Current architecture (simplified)

```mermaid
flowchart LR
    c0["User goal"]
    c1["Planning dialog"]
    c2["Open items"]
    c3["Spec update"]
    c4["Human decision"]
    c0 --> c1
    c1 --> c2
    c2 --> c3
    c3 --> c4
```

### Future architecture (target)

```mermaid
flowchart LR
    f0["Goal"]
    f1["Deduplicated question cards"]
    f2["Readiness gate"]
    f3["Approved snapshot"]
    f4["Adversarial plan review"]
    f0 --> f1
    f1 --> f2
    f2 --> f3
    f3 --> f4
```

## 06. Living specification and traceability

Target specification: [GitHub issue #161](https://github.com/zbarno/kool.ade/issues/161).

### Current architecture (simplified)

```mermaid
flowchart LR
    c0["Product specs"]
    c1["Change specs"]
    c2["Task specs"]
    c3["Implementation evidence"]
    c4["Git links"]
    c0 --> c1
    c1 --> c2
    c2 --> c3
    c3 --> c4
```

### Future architecture (target)

```mermaid
flowchart LR
    f0["Approved intent"]
    f1["Spec delta queue"]
    f2["Verified current behavior"]
    f3["Findings"]
    f4["Merge/deploy evidence"]
    f0 --> f1
    f1 --> f2
    f2 --> f3
    f3 --> f4
```

## 07. Shared planning and multi-user

Target specification: [GitHub issue #121](https://github.com/zbarno/kool.ade/issues/121).

### Current architecture (simplified)

```mermaid
flowchart LR
    c0["Shared Git planning"]
    c1["Per-record state"]
    c2["Operator local runtime"]
    c3["Code clones"]
    c4["Remote Git claims"]
    c0 --> c1
    c1 --> c2
    c2 --> c3
    c3 --> c4
```

### Future architecture (target)

```mermaid
flowchart LR
    f0["Planning remote"]
    f1["Optimistic revision checks"]
    f2["User A"]
    f3["User B"]
    f4["Global lease"]
    f5["Independent code repos"]
    f0 --> f1
    f1 --> f2
    f2 --> f3
    f3 --> f4
    f4 --> f5
```

## 08. Task DAG and clones

Target specification: [GitHub issue #162](https://github.com/zbarno/kool.ade/issues/162).

### Current architecture (simplified)

```mermaid
flowchart LR
    c0["Board work item"]
    c1["Task clone"]
    c2["CLI work"]
    c3["Git claim"]
    c4["Review/PR"]
    c0 --> c1
    c1 --> c2
    c2 --> c3
    c3 --> c4
```

### Future architecture (target)

```mermaid
flowchart LR
    f0["Approved TaskGraph"]
    f1["Runnable DAG nodes"]
    f2["Isolated subtask sessions"]
    f3["Parent clone integration"]
    f4["One PR"]
    f0 --> f1
    f1 --> f2
    f2 --> f3
    f3 --> f4
```

## 09. Local execution and sandbox

Target specification: [GitHub issue #136](https://github.com/zbarno/kool.ade/issues/136).

### Current architecture (simplified)

```mermaid
flowchart LR
    c0["CLI provider"]
    c1["Private MCP server"]
    c2["Bubblewrap"]
    c3["Bounded repository commands"]
    c4["Dependency broker"]
    c0 --> c1
    c1 --> c2
    c2 --> c3
    c3 --> c4
```

### Future architecture (target)

```mermaid
flowchart LR
    f0["Controller policy"]
    f1["ExecutionRequestV1"]
    f2["Local Docker backend"]
    f3["Non-root container"]
    f4["Evidence return"]
    f0 --> f1
    f1 --> f2
    f2 --> f3
    f3 --> f4
```

## 10. Remote execution parity

Target specification: [GitHub issue #139](https://github.com/zbarno/kool.ade/issues/139).

### Current architecture (simplified)

```mermaid
flowchart LR
    c0["Local CLI controller"]
    c1["Local clone"]
    c2["Local Bubblewrap sandbox"]
    c3["Local evidence"]
    c0 --> c1
    c1 --> c2
    c2 --> c3
```

### Future architecture (target)

```mermaid
flowchart LR
    f0["Controller"]
    f1["SSH authenticated worker"]
    f2["Version negotiation"]
    f3["Remote Docker container"]
    f4["Evidence synchronization"]
    f0 --> f1
    f1 --> f2
    f2 --> f3
    f3 --> f4
```

## 11. MCP and security boundaries

Target specification: [GitHub issue #156](https://github.com/zbarno/kool.ade/issues/156).

### Current architecture (simplified)

```mermaid
flowchart LR
    c0["CLI tool"]
    c1["Private MCP bridge"]
    c2["Scoped tool access"]
    c3["Bubblewrap"]
    c4["Application Git publisher"]
    c0 --> c1
    c1 --> c2
    c2 --> c3
    c3 --> c4
```

### Future architecture (target)

```mermaid
flowchart LR
    f0["CLI tool"]
    f1["Application policy boundary"]
    f2["Docker command bridge"]
    f3["Brokered resources"]
    f4["Controller Git publisher"]
    f0 --> f1
    f1 --> f2
    f2 --> f3
    f3 --> f4
```

## 12. Independent review and QA

Target specification: [GitHub issue #166](https://github.com/zbarno/kool.ade/issues/166).

### Current architecture (simplified)

```mermaid
flowchart LR
    c0["Implementation diff"]
    c1["Unit tests"]
    c2["Validation report"]
    c3["Human PR approval"]
    c0 --> c1
    c1 --> c2
    c2 --> c3
```

### Future architecture (target)

```mermaid
flowchart LR
    f0["Implementation diff"]
    f1["Independent code review"]
    f2["Desktop/browser QA"]
    f3["Finding"]
    f4["Repair"]
    f5["Fresh review"]
    f0 --> f1
    f1 --> f2
    f2 --> f3
    f3 --> f4
    f4 --> f5
```

## 13. PR, merge and delivery truth

Target specification: [GitHub issue #164](https://github.com/zbarno/kool.ade/issues/164).

### Current architecture (simplified)

```mermaid
flowchart LR
    c0["Task clone"]
    c1["Verification"]
    c2["PR preparation"]
    c3["Git remote"]
    c4["Board state"]
    c0 --> c1
    c1 --> c2
    c2 --> c3
    c3 --> c4
```

### Future architecture (target)

```mermaid
flowchart LR
    f0["Verified parent diff"]
    f1["Controlled PR"]
    f2["Merged code"]
    f3["Spec reconciliation"]
    f4["Deployment evidence"]
    f0 --> f1
    f1 --> f2
    f2 --> f3
    f3 --> f4
```

## 14. Persistence and telemetry

Target specification: [GitHub issue #161](https://github.com/zbarno/kool.ade/issues/161).

### Current architecture (simplified)

```mermaid
flowchart LR
    c0["Git planning"]
    c1["Operator conversations"]
    c2["Implementation records"]
    c3["Token ledger"]
    c4["Task report"]
    c0 --> c1
    c1 --> c2
    c2 --> c3
    c3 --> c4
```

### Future architecture (target)

```mermaid
flowchart LR
    f0["Planning remote"]
    f1["Immutable approvals"]
    f2["Local event ledger"]
    f3["Evidence manifest"]
    f4["Cost report"]
    f0 --> f1
    f1 --> f2
    f2 --> f3
    f3 --> f4
```

## 15. UI conversations and attention

Target specification: [GitHub issue #165](https://github.com/zbarno/kool.ade/issues/165).

### Current architecture (simplified)

```mermaid
flowchart LR
    c0["Board"]
    c1["Task Details"]
    c2["Task thread"]
    c3["Needs Attention"]
    c4["Specification chat"]
    c0 --> c1
    c1 --> c2
    c2 --> c3
    c3 --> c4
```

### Future architecture (target)

```mermaid
flowchart LR
    f0["Board projections"]
    f1["Question card"]
    f2["Approval card"]
    f3["QA review"]
    f4["PR card"]
    f5["Spec status"]
    f0 --> f1
    f1 --> f2
    f2 --> f3
    f3 --> f4
    f4 --> f5
```

## 16. Evolve and self-improvement

Target specification: [GitHub issue #174](https://github.com/zbarno/kool.ade/issues/174).

### Current architecture (simplified)

```mermaid
flowchart LR
    c0["Observed friction"]
    c1["Manual enhancement idea"]
    c2["User task"]
    c3["Implementation"]
    c4["PR"]
    c0 --> c1
    c1 --> c2
    c2 --> c3
    c3 --> c4
```

### Future architecture (target)

```mermaid
flowchart LR
    f0["Event observations"]
    f1["Opportunity proposal"]
    f2["Explicit opt-in"]
    f3["Isolated implementation"]
    f4["QA"]
    f5["Reviewed PR"]
    f0 --> f1
    f1 --> f2
    f2 --> f3
    f3 --> f4
    f4 --> f5
```

## 17. Stack-neutral extensibility

Target specification: [GitHub issue #173](https://github.com/zbarno/kool.ade/issues/173).

### Current architecture (simplified)

```mermaid
flowchart LR
    c0["User repo"]
    c1["Detected project metadata"]
    c2["Configured CLI"]
    c3["Explicit tests"]
    c4["Verification"]
    c0 --> c1
    c1 --> c2
    c2 --> c3
    c3 --> c4
```

### Future architecture (target)

```mermaid
flowchart LR
    f0["Capability detection"]
    f1["Unknown technology"]
    f2["Approval request"]
    f3["Extension implementation"]
    f4["Build/test/start recipes"]
    f0 --> f1
    f1 --> f2
    f2 --> f3
    f3 --> f4
```

## 18. Integrated happy path

Target specification: [GitHub issue #172](https://github.com/zbarno/kool.ade/issues/172).

### Current architecture (simplified)

```mermaid
flowchart LR
    c0["User input"]
    c1["Planning"]
    c2["Task clone"]
    c3["Agent implementation"]
    c4["Verification"]
    c5["PR"]
    c0 --> c1
    c1 --> c2
    c2 --> c3
    c3 --> c4
    c4 --> c5
```

### Future architecture (target)

```mermaid
flowchart LR
    f0["Goal"]
    f1["Questions"]
    f2["Approval"]
    f3["Plan review"]
    f4["Task DAG"]
    f5["Docker workers"]
    f6["Review and runtime QA"]
    f7["PR and spec sync"]
    f0 --> f1
    f1 --> f2
    f2 --> f3
    f3 --> f4
    f4 --> f5
    f5 --> f6
    f6 --> f7
```
