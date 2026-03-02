# 02 — Atomicity Model

## The Problem

Agentic systems today operate in long, multi-step chains. When step 12 of 20 fails, the system is left in an indeterminate state: some files modified, some not, some dependencies partially satisfied. Debugging this is brutal. Rolling back is manual. Retrying from scratch wastes all the successful work.

Databases solved this decades ago with ACID transactions. We need the same guarantees for agentic pipelines.

## The Output: Deterministic Micro-Apps, Not Generated Text

A critical distinction: the output of every atomic task is a **small, self-contained application** — not a generative response. The LLM is used during the build phase to produce working software. Once built and verified, that software runs deterministically with zero LLM involvement.

This means:
- **No generative output in production.** The agent builds a program. The program runs on its own.
- **No inference cost at runtime.** The pipeline is a factory — the products it manufactures are traditional software.
- **Fully testable and auditable.** Each micro-app is a real program that can be unit tested, profiled, debugged, and inspected like any other code.
- **No model dependency after build.** If the LLM provider goes down, already-built apps keep running. The pipeline stops producing new apps, but existing ones are unaffected.
- **Composability.** Micro-apps are composed into larger systems via standard interfaces (CLI args, stdin/stdout, HTTP, message queues) — not via prompt chaining.

The agent is the builder. The app is the product. The product never calls an LLM.

## Defining Atomic in This Context

An **atomic task** in this system has the following properties:

### 1. Complete or Nothing
The task either produces its full expected output and passes verification, or it produces nothing. There is no partial output that enters the system. Failed attempts are logged for diagnostics but their artifacts are discarded.

### 2. Self-Contained
The task can be understood and executed using only its context packet. It does not require implicit knowledge, ambient state, or assumptions about what other tasks have or haven't completed. All dependencies are declared and resolved before execution begins.

### 3. Idempotent
Running the same task twice with the same inputs produces functionally equivalent output. This enables safe retries. The output may not be identical (LLMs are non-deterministic) but it must be functionally equivalent — same interfaces, same behavior, same contract compliance.

### 4. Verifiable
Every atomic task has a corresponding verification step that can confirm success or failure without human intervention. Verification is cheaper than implementation. If you can't define verification criteria, the task is not well-specified.

## Task Anatomy

```yaml
atomic_task:
  id: unique-task-identifier
  source_doc:
    doc_id: reference to source spec
    doc_version: version of spec this was decomposed from
    section: specific section(s) of the spec this implements

  scope:
    target: file path, function name, or module
    operation: create | modify | delete
    boundary: what this task touches and nothing else

  context_packet:
    spec_section: extracted text from source doc relevant to this task
    interface_contracts: contracts of components this task interacts with
    type_definitions: relevant type/schema definitions
    examples: reference implementations or patterns to follow
    constraints: performance, style, convention requirements

  dependencies:
    required_before: [task IDs that must complete before this can start]
    provides_for: [task IDs that depend on this task's output]

  verification:
    type_check: boolean — must pass type checking
    tests: list of test cases or test generation instructions
    lint: boolean — must pass linting
    spec_compliance: specific invariants from the doc to assert
    review_prompt: prompt for a verification agent to review output against spec

  retry_policy:
    max_attempts: number (default 3)
    backoff: strategy between retries
    escalation: what happens after max_attempts exhausted
    variation_strategy: how to vary approach between retries
      # e.g., different temperature, different model, different prompt framing

  output:
    artifacts: [file paths or code blocks produced]
    verification_results: pass/fail for each verification criterion
    status: pending | running | passed | failed | escalated
```

## Boundaries: What Is and Isn't Atomic

### Good Atomic Tasks
- Build a small app that fetches and normalizes data from a single API endpoint on a schedule
- Build a CLI tool that validates a config file against a schema and exits with appropriate codes
- Implement a single-purpose module with a defined interface contract (e.g., a rate limiter, a retry wrapper, a data transformer)
- Write unit tests for a single module based on acceptance criteria in the spec
- Create a type definition file from interface contracts
- Build a standalone migration script that transforms data from schema v1 to v2

### Bad Atomic Tasks (Need Further Decomposition)
- "Implement the payment system" — too broad, multiple apps and contracts involved
- "Refactor the API layer" — unbounded scope, unclear completion criteria
- "Set up the database" — multiple steps (schema, migrations, seed data, connection config) each of which is a separate micro-app or script
- "Build the UI for settings" — involves layout, state management, API integration, validation — each is a separate task producing a separate component

### The Litmus Test
Ask: "Can this task be verified by a single, automated check?" If verification requires checking multiple unrelated things, the task should be split.

## The Task Graph (DAG)

Atomic tasks form a directed acyclic graph based on their dependencies.

```
[Types & Interfaces]
        |
   ┌────┴────┐
   ↓         ↓
[Module A] [Module B]
   |         |
   ↓         ↓
[Tests A]  [Tests B]
   |         |
   └────┬────┘
        ↓
  [Integration Tests]
        ↓
  [Verification Gate]
```

Rules for the DAG:

- **No cycles.** If A depends on B and B depends on A, the spec is wrong — go back to docs.
- **Maximum parallelism.** Independent tasks should have no dependency relationship. The orchestrator runs them concurrently.
- **Types and interfaces first.** The first tasks in any graph should produce the shared contracts (types, interfaces, schemas) that downstream tasks depend on. These are the cheapest to produce and the most leveraged.
- **Tests alongside or immediately after implementation.** Tests are separate atomic tasks but are tightly coupled to their implementation task. They use the same spec section but produce different artifacts.

## Failure and Rollback

### Single Task Failure
1. Task output is quarantined (kept for diagnostics but not committed)
2. Task is retried per its retry policy with variation strategy applied
3. If retries exhausted → task marked `escalated`
4. Dependent tasks are paused (not cancelled — the dependency might resolve)
5. Escalation triggers a review: is the spec ambiguous? Is the task decomposition wrong?

### Cascade Failure
If multiple tasks in the same subgraph fail:
1. Pause the entire subgraph
2. Analyze failure patterns — are they hitting the same issue?
3. Likely cause is a problem in a shared dependency or in the spec itself
4. Escalate to the doc layer for the relevant section

### Rollback Semantics
Because tasks are atomic and verified before commit, rollback is straightforward:
- **Uncommitted task:** Nothing to roll back — output was never merged
- **Committed task that needs reversal:** Generate a compensating task (delete the file, revert the change) — this is itself an atomic task
- **Spec change after partial implementation:** Identify all tasks derived from changed spec sections, mark their outputs as stale, regenerate tasks from updated spec

## Saga Pattern Adaptation

For complex workflows that span multiple atomic tasks, the system uses a saga pattern:

Each task in a saga has:
- **Action:** The implementation step
- **Compensation:** How to undo the action if a later step fails
- **Verification:** How to confirm the action succeeded

If step 5 of a 10-step saga fails, steps 1-4's compensations are executed in reverse order. The system returns to a clean state and the saga can be retried or escalated.

This is where workflow orchestration tools like Temporal become valuable — they provide saga semantics out of the box with durable execution guarantees.

## Idempotency Considerations

LLMs are non-deterministic. Running the same prompt twice produces different text. So what does idempotency mean here?

**Functional idempotency, not textual idempotency.** Two outputs are functionally idempotent if:
- They implement the same interface contract
- They pass the same verification suite
- They maintain the same invariants

Variable names might differ. Code structure might differ. Comments will differ. But the behavior is equivalent. This is why verification is critical — it's the definition of "same result."

For tasks where exact reproducibility matters (config files, schema definitions), use templating rather than generation. Don't ask an LLM to generate a JSON schema — generate it deterministically from the spec's type definitions.
