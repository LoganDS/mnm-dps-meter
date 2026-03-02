# Agentic Pipeline Architecture: Docs-First Atomic Implementation

## Vision

A multi-agent software development pipeline where human intent flows through iteratively refined documentation before being decomposed into atomic implementation tasks executed across a distributed compute cluster.

The core principle: **documentation is the program in a higher-level language.** Implementation is a constrained translation step, not a creative one.

## Architecture Layers

```
  ┌─────────────────────────────────────────────────┐
  │  0. AGENT GATEWAY                               │
  │     Single conversational agent (OpenClaw)      │
  │     Phone-accessible (Discord/WhatsApp/Signal)  │
  │     Only human-facing interface to the system   │
  └──────────────────────┬──────────────────────────┘
                         │
  BUILD PHASE (agents active)
┌────────────────────────┴────────────────────────┐
│  1. HUMAN INTENT                                │
│     Conversation via gateway                    │
├─────────────────────────────────────────────────┤
│  2. DOC AUTHORING & REFINEMENT                  │
│     Human + Agent collaborative drafting        │
│     Schema-enforced structured documents        │
│     Iterative challenge/refine cycles           │
│     Preferences elicitation (gray area capture) │
├─────────────────────────────────────────────────┤
│  3. DECOMPOSITION                               │
│     Docs + decisions.md → Task Graph (DAG)      │
│     Context partitioning per task               │
│     Dependency declaration + size budgets        │
├─────────────────────────────────────────────────┤
│  4. ATOMIC TASK QUEUE                           │
│     Self-contained task packets                 │
│     Scoped context bundles (within budget)      │
│     Verification criteria attached              │
│     Pre-execution review gate (READY/REVISION)  │
├─────────────────────────────────────────────────┤
│  5. IMPLEMENTATION AGENT POOL                   │
│     Mac Mini M4 cluster                         │
│     Local models + API hybrid                   │
│     Wave-based parallel execution (MVP)         │
│     Full DAG resolution (target)                │
├─────────────────────────────────────────────────┤
│  6. VERIFICATION & COMMIT                       │
│     Type check, test, lint, spec compliance     │
│     Runtime independence check (no LLM at run)  │
│     Pass → merge / Fail → retry or escalate     │
│     Human acceptance testing (when required)     │
└──────────────────────┬──────────────────────────┘
                       │
          micro-apps produced ↓
                       │
  RUNTIME PHASE (no agents — deterministic software)
┌──────────────────────┴──────────────────────────┐
│  7. COMPOSITION & WIRING                        │
│     Standard interface protocol (CLI + JSON)    │
│     Wiring manifest defines topology            │
│     Orchestrator coordinates execution          │
├─────────────────────────────────────────────────┤
│  8. TOOL REGISTRY                               │
│     Exposed tools registered with schemas       │
│     Discovery, invocation protocol, trust levels│
│     Gateway invokes tools as OpenClaw skills    │
├─────────────────────────────────────────────────┤
│  9. RUNTIME OPERATIONS                          │
│     Structured logging, metrics, trace IDs      │
│     Alerting, health checks                     │
│     Versioning, atomic swap, rollback           │
├─────────────────────────────────────────────────┤
│  10. FEEDBACK LOOP                              │
│     Runtime signals → doc amendments            │
│     Performance, failure, drift detection       │
│     Closes the loop back to layer 2             │
└──────────────────────┬──────────────────────────┘
                       │
                       └──→ back to GATEWAY (0) → human
```

## Key Design Principles

### 1. Agents Build Software, They Don't Run It
This is the foundational principle of the entire architecture. The pipeline's output is **deterministic, self-contained applications** — not generative LLM responses. Agents are the manufacturing line, not the product. Once an atomic micro-app is built, verified, and committed, it runs as traditional software with zero LLM involvement at runtime. There is no inference cost in production, no non-determinism in execution, and no dependence on model availability. The generative step exists only in the build phase. The outcome is always a working program, never a prompt result.

### 2. Docs as Source of Truth
The documentation layer is not a byproduct of development. It is the authoritative representation of system intent. Code is derived from docs, not the other way around. When code and docs diverge, the docs win and the code is regenerated.

### 3. Atomicity at Every Level
Every implementation task must be atomic: it either completes fully and passes verification, or it is discarded entirely. No partial state. No half-implemented features. This is enforced by the verification gate before any output is committed. Each atomic task produces a small, self-contained application or module that performs a single well-defined function. These micro-apps are composed into larger systems but remain independently runnable and testable.

### 4. Context as a Managed Resource
LLM context windows are treated like memory in a constrained system. Context is budgeted, scoped, routed, and versioned. No agent receives "everything" — each agent receives exactly the context packet required for its task.

### 5. Separation of Reasoning and Execution
High-reasoning tasks (doc refinement, decomposition, architectural decisions) are separated from mechanical execution tasks (implementing a well-specified function). This allows different models, different compute allocations, and different verification strategies for each.

### 6. Escalation Over Retry Loops
When an implementation agent fails repeatedly, the problem is escalated up to the doc layer rather than retried indefinitely. Repeated failure signals ambiguity in the spec, not incapability of the agent.

## Document Index

| Document | Purpose |
|----------|---------|
| [01-docs-first-workflow](./01-docs-first-workflow.md) | The documentation authoring and refinement process |
| [02-atomicity-model](./02-atomicity-model.md) | Atomic task definition, boundaries, and rollback semantics |
| [03-context-management](./03-context-management.md) | Context budgeting, routing, scoping, and compression strategies |
| [04-pipeline-orchestration](./04-pipeline-orchestration.md) | Task queue, dependency resolution, cluster execution model |
| [05-verification-and-escalation](./05-verification-and-escalation.md) | Commit gates, failure handling, escalation paths |
| [06-composition-and-wiring](./06-composition-and-wiring.md) | How micro-apps connect: interface protocol, wiring manifests, composition patterns |
| [07-tool-registry-and-agent-integration](./07-tool-registry-and-agent-integration.md) | How micro-apps become agent tools: registry, discovery, invocation protocol, trust levels |
| [08-runtime-operations](./08-runtime-operations.md) | Observability, alerting, versioning, updates, and the runtime-to-build feedback loop |
| [09-agent-gateway](./09-agent-gateway.md) | Single conversational interface to the pipeline: routing, trust, OpenClaw integration |
| [ralph-comparison-and-learnings](./ralph-comparison-and-learnings.md) | Deep analysis of the Ralph autonomous build pattern and 6 concrete adaptations |
| [gsd-comparison-and-learnings](./gsd-comparison-and-learnings.md) | Deep analysis of GSD context engineering system and 6 concrete adaptations |
| [spec-market-watcher](./spec-market-watcher.md) | First project spec: 15-minute market data collection system |

## Resolved Design Decisions

### Doc schema format
**Markdown.** Human-readable, git-friendly, every LLM handles it natively, and diffs are clean. Structured sections within markdown (using YAML code blocks for schemas, consistent heading hierarchy for machine parsing) give us the parseability we need without sacrificing readability.

### Local model vs API task boundary
**The tier routing logic is the boundary, tuned empirically based on first-pass success rates per tier.** There is no fixed line — task characteristics (reasoning complexity, context budget, interface clarity) determine the tier assignment at decomposition time. If local models are failing too often on a task type, that type gets promoted to API. If API tasks are trivially easy, they get demoted to local. The tier system defined in doc 04 captures the routing logic. See: [04-pipeline-orchestration](./04-pipeline-orchestration.md).

### Emergent cross-cutting concerns
**The escalation path handles these reactively; the challenge cycle prevents them proactively.** When multiple tasks in different branches of the DAG fail for related reasons, that's a cascade failure signal. It escalates back to the doc layer, and the cross-cutting concern gets its own spec (a "shared conventions" doc). Preventively, the challenge cycle in doc refinement should explicitly probe: "what do all components in this system need to agree on that isn't captured in any individual component's spec?" — things like error formats, logging conventions, config patterns. Docs 06 and 08 capture many of these shared conventions. Each new system spec should include a "shared conventions" section.

### Minimum viable version
**Build Market Watcher using a minimal, manual version of the pipeline.** The MVP is the first real project, not the pipeline automation. Manually write specs (skip agent-assisted doc refinement), manually decompose into tasks (skip the decomposition agent), have one agent (Claude via API) build the micro-apps one at a time, run basic verification (does it run, do tests pass), and manually compose them. The pipeline automation comes after validating that the doc schema, atomic task format, and interface protocol work in practice. Build the product before you build the factory.

### Maximum tool chain depth
**3 without review, 5 with a logged plan, anything beyond 5 requires human approval.** At depth 3, the agent is doing something straightforward and traceable (query → transform → write). At depth 5, subtle errors can compound through the chain non-obviously. Beyond 5, the agent is essentially running an ad-hoc program it designed on the fly — which violates the principle that programs get specified and verified before they run. If an agent needs a chain longer than 5, that chain should become its own spec'd composition. See: [07-tool-registry-and-agent-integration](./07-tool-registry-and-agent-integration.md).

### Feedback loop signal prioritization
**Severity first, then frequency, then age — with correlation grouping.** A critical alert (system not collecting data) always trumps a warning (one symbol failing). Among same-severity signals, the most frequent is likely most impactful. Among same-severity, same-frequency signals, the oldest has been degrading the system longest. Correlated signals are grouped before surfacing: if 5 symbols fail with the same error code at the same time, that's one issue (probably provider-side), not 5 separate issues. The health checker should do basic correlation — group by error code, group by time window, group by affected component. See: [08-runtime-operations](./08-runtime-operations.md).

### Tool registry versioning
**Default to "latest" with the option to pin when needed.** Agents resolve tool references to the latest active version unless a specific version is pinned in a wiring manifest or task spec. Pinning is available for production compositions that need stability guarantees, but the default is latest to keep things simple and ensure agents always use the most current, verified version. See: [07-tool-registry-and-agent-integration](./07-tool-registry-and-agent-integration.md).

## Adaptations from the Ralph Pattern

The [Ralph autonomous build loop](https://github.com/snarktank/ralph) and its Goose cross-model variant validated several patterns that are now incorporated into this architecture. See [ralph-comparison-and-learnings](./ralph-comparison-and-learnings.md) for the full analysis.

### Build Memory (patterns.md + build-log.md)
Each project maintains a curated `patterns.md` file of implementation learnings (API quirks, library conventions, gotchas) that every implementation agent reads as part of its context bundle. An append-only `build-log.md` captures the raw activity of each build run. Patterns are promoted from the build log; the curated file is what agents see. See: [03-context-management](./03-context-management.md) "Build Memory" section.

### Structured Verification Verdicts (SHIP / REVISE / ESCALATE)
Every verification pass produces a structured verdict with a decision, detailed feedback, affected files, and a retry hint. REVISE verdicts carry specific guidance that is included in the retry task's context bundle, giving the next attempt precise information about what went wrong and how to fix it. See: [05-verification-and-escalation](./05-verification-and-escalation.md) "Structured Verification Verdicts" section.

### Cross-Model Verification
The verification agent must use a different model than the implementation agent for spec compliance checks (Layer 3). Different models have different blind spots — cross-model review catches issues that self-review misses. See: [05-verification-and-escalation](./05-verification-and-escalation.md) "Key Design Choices."

### Acceptance Criteria on Task Packets
Each atomic task carries both formal verification criteria (typecheck, tests, spec compliance) and plain-language acceptance criteria — a simple checklist the implementation agent uses to self-test before submitting. See: [04-pipeline-orchestration](./04-pipeline-orchestration.md) "Task Packet Schema."

### Ralph-Compatible Export
The decomposition phase can export the task DAG as a Ralph-compatible `prd.json` file, enabling Ralph as the MVP execution engine while the full DAG orchestrator is being built. See: [04-pipeline-orchestration](./04-pipeline-orchestration.md) "Ralph-Compatible Export."

## Adaptations from the GSD Pattern

The [GSD (Get Shit Done)](https://github.com/glittercowboy/gsd) context engineering system validated several patterns that are now incorporated into this architecture. See [gsd-comparison-and-learnings](./gsd-comparison-and-learnings.md) for the full analysis.

### Preferences Elicitation (decisions.md)
Before decomposition, the system analyzes the approved spec for unresolved preference decisions — gray areas where multiple valid approaches exist. The discuss step captures user intent on these choices and records them in `decisions.md`, which feeds into decomposition alongside the spec. This is distinct from the challenge cycle: challenges ask "is this right?", preferences elicitation asks "which of these valid options do you prefer?" See: [01-docs-first-workflow](./01-docs-first-workflow.md) "Preferences Elicitation" section.

### Pipeline Tools CLI
All mechanical pipeline operations (spec parsing, context assembly, state management, validation) are extracted into a single deterministic CLI tool. This keeps agent context focused on reasoning rather than file manipulation, saving 5,000-10,000 tokens per workflow. See: [03-context-management](./03-context-management.md) "Pipeline Tools CLI" section.

### Document Size Budgets
Every document type has an explicit size budget. Specs max at ~2,000 lines, task packets at ~200 lines, context bundles at tier-appropriate limits. When a document exceeds its budget, that signals decomposition is needed — not a longer context window. See: [03-context-management](./03-context-management.md) "Document Size Budgets" section.

### Pre-Execution Plan Review
Before a task packet enters the implementation queue, a deterministic checklist verifies the packet is well-formed: acceptance criteria present and testable, context bundle complete, dependencies satisfied, size within budget. This catches bad decomposition before spending implementation tokens. See: [04-pipeline-orchestration](./04-pipeline-orchestration.md) "Pre-Execution Plan Review" section.

### Wave-Based MVP Orchestration
Tasks are grouped by dependency depth into waves. All tasks in a wave run in parallel; waves run sequentially. This captures 80% of the parallelism benefit with 20% of the orchestration complexity, serving as the MVP mode before full DAG resolution. See: [04-pipeline-orchestration](./04-pipeline-orchestration.md) "Wave-Based MVP Orchestration" section.

### Pipeline State and Pause/Resume
Each build run maintains a `pipeline-state.yaml` tracking task statuses, current wave, and pause reason. The orchestrator can be cleanly interrupted and resumed from this state file, preventing lost work. See: [04-pipeline-orchestration](./04-pipeline-orchestration.md) "Pipeline State Management" section.

## Updated MVP Execution Path

With both Ralph and GSD learnings incorporated, the full build execution path is:

```
1. Finalize spec (challenge cycle) — core process
2. Gray area identification / preferences elicitation — from GSD discuss pattern
3. Decompose into atomic tasks with size budgets — core process + GSD size limits
4. Pre-execution plan review — from GSD plan-checker
5. Add acceptance criteria to each task — from Ralph
6. Create pipeline-state.yaml — from GSD pause/resume
7. Wave-based execution — from GSD, using Ralph as inner loop
8. After each task: update patterns.md, append build-log — from Ralph
9. Verification: cross-model spec compliance — from Goose/Ralph
10. Build log captures everything — from Ralph
11. Compose and deploy as Docker containers — core process
12. Gateway monitors runtime — core process
```

Steps 1, 3, 11-12 are core architecture. Steps 5, 8-10 are Ralph adaptations. Steps 2, 4, 6-7 are GSD adaptations. The result is the architecture's rigor with Ralph's proven execution mechanics and GSD's context engineering discipline.
