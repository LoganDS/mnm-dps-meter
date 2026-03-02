# 01 — Docs-First Workflow

## Core Concept

Traditional agentic coding asks an LLM to go from intent to implementation in one leap. This produces brittle, hard-to-verify output because the agent is simultaneously making architectural decisions, resolving ambiguity, and writing code.

The docs-first pattern separates these concerns. **Iterate on the "what" before attempting the "how."**

Critically, the "how" always results in a **small, self-contained, deterministic application** — not a generative response. The docs describe what the app should do. The agent builds the app. The app runs on its own with zero LLM involvement. The doc-to-app pipeline is a manufacturing process: docs are the blueprints, agents are the factory, and runnable micro-apps are the product.

```
Intent → Draft Doc → Refine → Challenge → Refine → Preferences → Approve → Implement
          ↑___________________________↓                  ↓
              (iterate until stable)          decisions.md feeds
                                              into decomposition
```

The document is a checkpoint, a rollback point, and a contract. If implementation fails, you don't lose the thinking — you return to the doc and try again.
## Why This Works

### 1. Agents Are Better at Natural Language Iteration
LLMs excel at reasoning over, critiquing, and refining prose. They are worse at getting complex code correct on the first pass. Playing to the model's strengths means doing the hard thinking in the medium where the model is most capable.

### 2. Human Review is More Effective on Docs Than Code
A human can review a well-structured spec in minutes and catch fundamental design errors. Reviewing 2,000 lines of generated code for the same errors takes hours and requires deep technical context. Docs-first puts the human oversight where it has the highest leverage.

### 3. Docs Compress Context
A refined spec document is a compressed representation of intent. It encodes decisions that would otherwise need to be inferred from code, comments, commit history, and tribal knowledge. This compression is critical for managing context window limits downstream.

### 4. Docs Enable Parallel Implementation
A monolithic coding task is sequential. A well-decomposed spec with clear interface contracts enables parallel implementation by multiple agents — each building a separate micro-app from their section of the doc. Because each app is self-contained with defined interfaces, agents don't need to coordinate in real time. The doc is the coordination mechanism.
## Document Schema

Every spec document in the system must follow a structured schema. Free-form markdown is not sufficient — the schema ensures downstream agents can reliably extract what they need.
### Required Sections

```yaml
document:
  metadata:
    id: unique-identifier
    version: semantic version
    status: draft | review | approved | implementing | complete
    author: human or agent identifier
    dependencies: [list of doc IDs this depends on]
    last_refined: timestamp

  overview:
    purpose: what this component/feature does (1-2 paragraphs)
    scope: what is included and explicitly excluded
    context: where this fits in the broader system

  interface_contract:
    inputs:
      - name, type, constraints, source
    outputs:
      - name, type, guarantees
    side_effects:
      - description, reversibility
    error_conditions:
      - condition, expected behavior

  invariants:
    - statements that must always be true about this component
    - these become assertion targets and verification criteria

  acceptance_criteria:
    - specific, testable conditions for "done"
    - each criterion maps to a verification step

  output_artifact:
    type: micro-app | module | config | migration | test-suite
    description: what the built artifact does when run
    runtime_dependencies: what it needs to execute (runtime, database, env vars)
    interface: how it is invoked (CLI, function call, HTTP, cron trigger)
    deterministic: true — the output must run without LLM involvement

  dependencies:
    internal:
      - other components in the system this interacts with
      - interface contracts it relies on
    external:
      - third-party services, libraries, APIs

  design_decisions:
    - decision: what was decided
      rationale: why
      alternatives_considered: what else was evaluated
      reversibility: easy | moderate | difficult

  open_questions:
    - unresolved items that need human input before implementation

  decisions:
    - question: the gray area or preference that was surfaced
      decision: what was decided
      rationale: why this option was chosen
      source: challenge_cycle | preferences_elicitation | human_override

  size_budget:
    total: ~2000 lines
    per_section: ~300 lines
    signal_if_exceeded: "spec needs further decomposition into sub-specs"
```

### Why Schema Enforcement Matters

Without a schema, docs drift into vague narratives that give agents too much room for interpretation. The schema forces specificity:

- **Interface contracts** eliminate ambiguity about boundaries
- **Invariants** become test assertions
- **Acceptance criteria** become verification gates
- **Open questions** explicitly flag what cannot yet be implemented

If an agent cannot fill in a required section, that's a signal the spec needs more refinement — not that the section should be skipped.

## Refinement Process

### Phase 1: Human Draft
The human provides initial intent. This can be rough — conversational, incomplete, aspirational. The goal is to capture the core idea, not to be precise.

### Phase 2: Agent Structuring
An agent takes the rough intent and produces a first-pass structured doc following the schema. It fills in what it can and flags open questions where the intent is ambiguous.

### Phase 3: Challenge Cycle
A dedicated "challenger" agent (or the same agent in a different mode) reviews the structured doc and asks hard questions:

- What happens when [edge case]?
- This invariant conflicts with [other component's contract] — which wins?
- The acceptance criteria are not testable as written — how would you verify criterion 3?
- This dependency is not declared but is implied by the interface contract

The human reviews the challenges, answers what they can, and the doc is refined.

### Phase 4: Preferences Elicitation

After challenges have been addressed but before final approval, a dedicated step surfaces unresolved *preference* decisions — gray areas where multiple valid approaches exist and the choice depends on user intent rather than technical correctness.

This is distinct from the challenge cycle. Challenges ask "is this right?" Preferences elicitation asks "which of these valid options do you prefer?"

```yaml
preferences_elicitation:
  input: challenged and refined spec
  process:
    1. analyze spec for gray areas by component type:
       - data pipelines → error handling strategy, retry behavior, partial failure policy
       - CLI tools → output verbosity, flag conventions, help text style
       - system composition → logging strategy, config format, health check granularity
       - APIs → response pagination, rate limit communication, versioning convention
    2. present gray areas as explicit questions with suggested defaults
    3. capture decisions in a decisions.md file
  output: decisions.md (feeds into decomposition alongside spec)
  trigger: after at least one challenge cycle, before approval
```

**Why this matters:** Without explicit preference capture, agents guess — and guesses accumulate into inconsistency across micro-apps. A decision like "should the health checker log to stdout or to a structured log file?" seems minor in isolation, but when 8 micro-apps each make this choice independently, the system's operational surface becomes unpredictable.

The `decisions.md` file is versioned alongside the spec and included in the context bundle for decomposition. It does not replace the spec — it supplements it with preference data that would otherwise be implicit.

### Phase 5: Approval Gate
The doc is considered approved when:

- All required schema sections are populated
- No open questions remain (or remaining questions are explicitly deferred with justification)
- Interface contracts are consistent with dependent/depended-upon components
- At least one challenge cycle has been completed
- Preferences elicitation has been completed and decisions.md is populated (gray areas resolved or explicitly deferred)

Only approved docs proceed to decomposition. The decomposition agent receives both the approved spec and `decisions.md` as inputs.

## Versioning

Docs are versioned. When a doc is updated after implementation has begun:

1. The update is tagged with a new version
2. A diff is generated against the version used for active implementation tasks
3. Affected tasks are identified and flagged for re-evaluation
4. Tasks that are no longer valid are rolled back

This is the consistency guarantee that prevents context drift across the pipeline.

## Anti-Patterns to Avoid

- **Rubber-stamping:** Approving docs without genuine challenge cycles. The refinement process is where bugs are cheapest to fix.
- **Over-specification:** Specifying implementation details (use this library, structure the code this way) rather than behavior and contracts. Over-specification defeats the purpose of letting agents make implementation decisions.
- **Orphan docs:** Docs that exist but aren't linked to the dependency graph. Every doc should declare what it depends on and what depends on it.
- **Skipping to code:** The temptation to "just let the agent code it" when a task seems simple. Simple tasks still benefit from a lightweight spec — it takes 2 minutes and prevents 20 minutes of debugging.
- **Generative runtime:** Designing a component where an LLM is called at execution time to produce behavior. If the output requires an LLM to function, it is not a deterministic micro-app. The only acceptable LLM involvement is during the build phase. The product must run on its own.
