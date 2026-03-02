# 03 — Context Management Strategy

## The Constraint

Current frontier models offer ~200k token context windows. This sounds large but is consumed quickly in practice:

| Content | Typical Token Count |
|---------|-------------------|
| System prompt + instructions | 2-5k |
| Full spec document | 5-20k |
| Single source file | 1-5k |
| 10 related source files | 10-50k |
| Interface contracts for adjacent components | 5-15k |
| Examples and reference patterns | 5-10k |
| Conversation/iteration history | 10-40k |

A moderately complex implementation task with full context easily exceeds 100k tokens. And raw capacity isn't the only problem.

### The Fidelity Problem

Models attend unevenly across long contexts. Information in the middle of a large context window receives less attention than information at the beginning or end (the "lost in the middle" effect). Stuffing 150k tokens of context and hoping the model finds the relevant 5k is a losing strategy.

**Context management is not about fitting more in. It's about ensuring what's in there is high-signal and positioned for maximum attention.**

## Design Principle: Context as Cache Hierarchy

Treat context like a CPU cache hierarchy:

```
┌─────────────────────────┐
│  L1: Active Context     │  ← What's in the model's context window
│  (10-30k tokens)        │     High fidelity, task-specific
├─────────────────────────┤
│  L2: Doc Store          │  ← Structured specs, searchable
│  (all project docs)     │     Retrieved on demand per task
├─────────────────────────┤
│  L3: Full Codebase      │  ← Complete source, indexed
│  (entire repository)    │     Accessed via targeted retrieval
├─────────────────────────┤
│  L4: External Knowledge │  ← API docs, library references
│  (internet, docs sites) │     Fetched when needed
└─────────────────────────┘
```

The goal is to keep L1 small, precise, and task-relevant. Everything else lives in lower tiers and is pulled up only when needed.

## Context Budgeting

Every atomic task has a **context budget** — a target token count for its context packet. This is enforced at decomposition time.

### Budget Allocation Per Task

```
Total available context:   200k tokens
Reserved for system/model:  -5k
Reserved for output:       -30k  (model needs room to generate)
Reserved for overhead:     -15k  (formatting, delimiters, instructions)
────────────────────────────────
Usable for task context:   150k tokens (theoretical max)
Target per atomic task:    10-30k tokens (practical target)
```

Why target 10-30k when 150k is available? Because:
- Smaller context = higher attention fidelity
- Smaller context = faster inference
- Smaller context = lower cost (for API-based models)
- If a task needs more than 30k of context, it's probably not atomic enough
- Each task produces a self-contained micro-app — if the app needs the entire system's context to be built, it's not self-contained

### Budget Breakdown Within a Task

```yaml
context_budget:
  task_instructions: 1-2k     # What to do, output format
  spec_section: 3-8k          # Relevant portion of the source doc
  interface_contracts: 2-5k   # Inputs/outputs/types this task touches
  reference_code: 3-10k       # Existing code to modify or patterns to follow
  examples: 2-5k              # Example implementations of similar tasks
  constraints: 1-2k           # Style, performance, convention requirements
  project_patterns: 1-2k      # Accumulated implementation learnings
  retry_context: 0-2k         # Verdict from failed attempt (if retrying)
  ────────────────
  total target: 13-36k
```

## Context Routing

The decomposition agent is responsible for assembling each task's context packet. This is not a generic RAG retrieval — it's a deterministic routing based on the task's declared dependencies.

### Routing Rules

1. **Spec section:** The task declaration includes a reference to the specific doc section(s) it implements. Extract those sections verbatim.

2. **Interface contracts:** The task's dependency declarations identify which components it interacts with. Pull the interface contracts for those components from their respective docs.

3. **Type definitions:** If the task operates on shared types, include the relevant type definitions. These should be maintained as standalone docs/files that are easy to include.

4. **Reference code:** If the task modifies existing code, include the current state of the target file(s). If it creates new code, include one example of a similar component for pattern reference.

5. **Project patterns file:** If a `patterns.md` exists for the project, include it. This file contains implementation learnings accumulated from prior tasks — API quirks, database conventions, testing patterns, and gotchas discovered during the build. It's small (typically 1-2k tokens) and high-value, preventing the same mistakes across tasks. See the Build Memory section below.

6. **Retry context:** If this is a retry attempt, include the structured verification verdict from the failed attempt (verdict, feedback, retry hint, affected files). Do NOT include the full previous code — only the feedback. See doc 05 for the verdict schema.

7. **Nothing else.** Do not include "nice to have" context. Do not include the full spec when only one section is needed. Do not include the implementation of unrelated components.

### Context Packet Assembly

```
def assemble_context_packet(task):
    packet = {}

    # Always include
    packet['instructions'] = generate_task_instructions(task)
    packet['spec'] = extract_doc_section(task.source_doc, task.section)

    # Include based on dependencies
    for dep in task.dependencies.interfaces:
        packet[f'contract_{dep.id}'] = get_interface_contract(dep)

    for type_ref in task.dependencies.types:
        packet[f'type_{type_ref}'] = get_type_definition(type_ref)

    # Include if modifying existing code
    if task.scope.operation == 'modify':
        packet['current_code'] = read_file(task.scope.target)

    # Include one reference example if creating new code
    if task.scope.operation == 'create':
        packet['example'] = find_similar_component(task)

    # Include project patterns if they exist
    patterns_file = get_patterns_file(task.project)
    if patterns_file:
        packet['patterns'] = patterns_file

    # Include retry context if this is a retry attempt
    if task.attempt > 1:
        packet['retry_context'] = task.previous_verdict  # structured feedback only

    # Validate budget
    total_tokens = count_tokens(packet)
    if total_tokens > task.context_budget:
        raise ContextBudgetExceeded(task, total_tokens)

    return packet
```

## Structured Extraction Over Summarization

When context must be compressed (e.g., a spec section is too long for the budget), use **structured extraction** rather than summarization.

### Why Not Summarization

Summarization is lossy and unpredictable. The summarizing model decides what's "important," which may not align with what the downstream task actually needs. You lose specifics — exact types, exact error conditions, exact constraints — in favor of narrative overview.

### Structured Extraction

Instead of "summarize this spec section," ask:

```
From the following spec section, extract ONLY:
1. Input types and their constraints
2. Output types and their guarantees
3. Error conditions and expected behavior
4. Invariants that must hold
5. Performance or resource constraints

Output as structured YAML. Do not include narrative or explanation.
```

This produces a deterministic, dense context payload. It's lossy too, but the loss is controlled — you choose what fields to extract based on what the downstream task needs.

### When Summarization Is Acceptable

- Overview context: "What does the broader system do?" A 500-token summary of the architecture doc is fine for orientation.
- Decision rationale: "Why was this approach chosen?" Summarizing design decisions for context is reasonable since the downstream task doesn't depend on exact wording.
- Conversation history: Summarizing previous iteration rounds to retain key decisions without the full back-and-forth.

## Versioning and Consistency

### The Problem

Agent A decomposes a doc (version 3) into tasks. While tasks are in the queue, the doc is updated to version 4. Agent B picks up a task and receives a context packet assembled from version 3 of the spec but version 4 of an interface contract.

This is a distributed consistency problem.

### The Solution

Every context packet is stamped with the versions of all source documents used to assemble it.

```yaml
context_packet:
  version_manifest:
    source_doc: { id: "auth-service", version: "3.1.0" }
    contracts:
      - { id: "user-service-api", version: "2.0.0" }
      - { id: "token-store-api", version: "1.3.0" }
    types:
      - { id: "shared-types", version: "4.0.0" }
  assembled_at: timestamp
```

Before a task executes, the orchestrator checks: are all versions in this manifest still current? If any source has been updated, the task is re-evaluated:

- **Non-breaking change:** Context packet is reassembled with new version, task proceeds
- **Breaking change:** Task is invalidated and re-decomposed from the updated doc
- **Ambiguous:** Flagged for human review

## Context Positioning

Given the "lost in the middle" effect, the order of content in the context window matters.

### Recommended Ordering

```
1. Task instructions (what to do, output format)     ← START: high attention
2. Retry context (if retrying: what went wrong)
3. Interface contracts (inputs, outputs, types)
4. Constraints and invariants
5. Project patterns (accumulated learnings)
6. Spec section (the detailed requirements)           ← MIDDLE: lower attention
7. Reference code or examples
8. Acceptance criteria and verification expectations   ← END: high attention
```

The most critical information — what the task must do, what went wrong last time, and how it will be verified — is at the boundaries where attention is highest. The project patterns file sits between contracts and the spec, providing ambient context that influences implementation without dominating attention.

## Build Memory

Implementation learnings accumulate during the build process. Without a mechanism to capture and propagate them, each task starts from zero — and the same mistakes repeat across tasks.

Two files serve as build memory. Both live in the project repo alongside the spec.

### patterns.md — Curated Implementation Knowledge

A curated file of reusable patterns and gotchas for a project. Every implementation agent reads it as part of their context bundle.

```markdown
# Patterns — Market Watcher

## API Patterns
- Binance kline timestamps are in milliseconds, divide by 1000 for epoch seconds
- Rate limit weight for /api/v3/klines is 2 per request

## Database Patterns
- psycopg3 requires autocommit=True for DDL statements
- Use INSERT ... ON CONFLICT (symbol, timestamp) DO UPDATE for upserts

## Testing Patterns
- Mock Binance responses should include the full 12-element kline array
- Test Postgres container: use tmpfs for speed

## Gotchas
- BNB/USDT pair name on Binance is BNBUSDT, not BNB-USDT
- Empty kline response for off-hours is [], not null
```

**Lifecycle:**
1. Created empty when a spec is approved for implementation.
2. Updated by implementation agents — after a task passes verification, the agent appends any reusable learnings it discovered.
3. Reviewed periodically — stale entries are pruned, important entries are promoted to the spec itself if they represent permanent design knowledge.
4. Every task in the project includes patterns.md in its context bundle.

**What goes in:** General, reusable knowledge that would help future tasks in the same project. API quirks, library conventions, testing strategies, naming patterns.

**What does NOT go in:** Task-specific implementation details, code snippets, temporary workarounds. These go in the build log.

### build-log.md — Append-Only Build Activity

An append-only log of what happened during a build run. Captures implementation activity, timing, failures, and the learnings promoted to patterns.md.

```markdown
## [2026-02-28T03:15:00Z] Task: schema-manager — SHIPPED
- Files: schema_manager.py, migrations/001_create_candles.sql
- Duration: 4m 22s
- Attempts: 1
- Learnings: psycopg3 connection needs autocommit for CREATE TABLE
- Promoted to patterns.md: yes (Database Patterns)

## [2026-02-28T03:30:00Z] Task: binance-adapter — REVISED (attempt 1)
- Verdict: REVISE
- Feedback: Returns raw JSON list, not CandleRaw objects
- Retry hint: Parse kline array into CandleRaw dataclass before serialization

## [2026-02-28T03:38:00Z] Task: binance-adapter — SHIPPED (attempt 2)
- Files: binance_adapter.py, tests/test_binance_adapter.py
- Duration: 6m 44s (total across attempts)
- Attempts: 2
- Learnings: Binance kline arrays are 12 elements, not 11 — last element is "ignore"
- Promoted to patterns.md: yes (API Patterns)
```

**Lifecycle:**
1. Created when a build run starts.
2. Appended to after every task attempt (pass or fail).
3. Archived after the build run completes.
4. Not included in task context bundles (too large, too noisy). The curated patterns.md is the version agents see.

**Purpose:** Audit trail and raw data for pipeline improvement. Patterns in the build log (repeated failure types, slow tasks, common gotchas) feed into doc refinements and pipeline tuning.

### Repository Structure

```
project-repo/
├── spec-market-watcher.md       # The specification
├── patterns.md                   # Curated implementation knowledge (agents read this)
├── builds/
│   ├── build-2026-02-28.md      # Build log for this run
│   └── build-2026-03-01.md      # Build log for next run
└── src/
    └── ...                       # The implemented micro-apps
```

## Pipeline Tools CLI

All mechanical pipeline operations — parsing, validation, state management, context assembly — are extracted into a single deterministic CLI tool. Implementation agents call this CLI for mechanical operations rather than doing inline parsing in their context windows.

This is a direct adaptation of GSD's `gsd-tools.cjs` pattern. Moving deterministic operations out of agent context saves 5,000-10,000 tokens per workflow and eliminates errors from inline parsing logic.

### Tool Responsibilities

```yaml
pipeline-tools:
  spec_operations:
    - parse-spec: extract sections, validate schema compliance
    - extract-contracts: pull interface contracts from spec for context bundles
    - extract-invariants: pull invariants for verification criteria
    - validate-dependencies: check cross-spec dependency consistency
    - extract-decisions: pull decisions.md entries relevant to a specific task

  task_operations:
    - assemble-context: build context bundle for a task from spec + patterns + deps + decisions
    - export-prd-json: generate Ralph-compatible export
    - init-task-packet: create task packet skeleton from spec decomposition
    - validate-packet: run pre-execution review checklist (see doc 04)

  state_operations:
    - update-pipeline-state: record task completion/failure in pipeline-state.yaml
    - append-build-log: add entry to build-log.md
    - update-patterns: propose a pattern for patterns.md
    - get-build-status: return current pipeline state for resume decisions

  wiring_operations:
    - validate-wiring: check wiring manifest for consistency
    - generate-compose: produce Docker Compose from wiring manifest
```

### Invocation Convention

```bash
# Extract contracts for a specific task's dependencies
pipeline-tools extract-contracts --spec spec-market-watcher.md --task binance-adapter

# Assemble a complete context bundle
pipeline-tools assemble-context --task binance-adapter --tier 2

# Update pipeline state after task completion
pipeline-tools update-pipeline-state --task binance-adapter --status passed --attempt 1
```

The CLI returns structured JSON, following the same protocol as the micro-apps it helps build. Agents treat it as a tool call, not inline code.

### Build Priority

The pipeline-tools CLI is built incrementally alongside the first project. Start with `assemble-context` and `update-pipeline-state` (highest value), then add spec parsing and validation operations as the pipeline matures.

## Document Size Budgets

Every document type in the pipeline has an explicit size budget. These limits exist because output quality degrades predictably above certain context thresholds. When a document exceeds its budget, that's a signal it needs decomposition — not a longer context window.

```yaml
document_budgets:
  spec_document:
    total: ~2000 lines
    per_section: ~300 lines
    signal_if_exceeded: "spec needs further decomposition into sub-specs"

  context_bundle:
    total: varies by model tier
    local_model: ~800 lines (~32k tokens)
    api_model: ~3000 lines (~128k tokens)
    signal_if_exceeded: "task needs further decomposition"

  task_packet:
    total: ~200 lines
    acceptance_criteria: ~10 items
    signal_if_exceeded: "task is not atomic"

  patterns_md:
    total: ~200 lines
    signal_if_exceeded: "prune stale patterns, split by domain"

  decisions_md:
    total: ~100 lines
    signal_if_exceeded: "too many unresolved preferences — spec may need restructuring"

  build_log:
    total: unbounded (append-only)
    per_entry: ~10 lines

  pipeline_state:
    total: ~300 lines
    signal_if_exceeded: "build has too many tasks — consider phased execution"
```

### Enforcement

Size budgets are checked at two points:

1. **At decomposition time:** The decomposition agent validates that each produced task packet and its assembled context bundle fit within budget. If not, the task is further decomposed before entering the queue.
2. **At execution time:** The pipeline-tools CLI validates the assembled context bundle against the target model tier's budget before dispatch. Over-budget packets are rejected back to decomposition.

Budget violations are never silently ignored — they always produce a signal that triggers decomposition or pruning.

## Anti-Patterns

- **Context stuffing:** Including everything "just in case." More context ≠ better results.
- **Flat retrieval:** Using generic semantic search to find "relevant" chunks. Context routing should be deterministic based on declared dependencies, not vibes-based similarity.
- **Stale context:** Assembling context packets in advance and queuing them. Context should be assembled at execution time to ensure freshness.
- **Ignoring token budgets:** Letting tasks consume 100k+ tokens because "the model supports it." Budget enforcement keeps tasks focused and catches decomposition failures.
- **Recursive summarization:** Summarizing a summary of a summary. Each pass loses fidelity exponentially. If you need to compress that much, the original is too large for one task.
- **Confusing build context with runtime context.** The context management strategy applies to the agent build phase only. The micro-apps produced by the pipeline do not consume LLM context at runtime — they are deterministic software. If a task's output would require LLM context to function, the task is mis-specified.
