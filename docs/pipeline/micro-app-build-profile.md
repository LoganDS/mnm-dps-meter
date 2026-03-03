# Micro-App Build Profile — Standard Template
## Version: 3.0 (parallel waves, no worktrees)

Lessons incorporated from: Market Watcher (13 tasks), Alpaca Adapter (6 tasks),
Earnings Calendar (7 tasks), Signal Engine (7 tasks), Trading Engine (9 tasks),
CCXT Adapter (6 tasks — worktree failures prompted v3 rewrite).

**v3 change:** Removed worktree-based parallelism. Parallel tasks now run in the
shared working directory since wave decomposition guarantees no file conflicts.
Each wave ends with `git add -A && git commit` instead of branch merges.
Sequential waves still commit per-task. This eliminates merge helpers, stale
branches, and ghost-process merge failures while keeping wall-clock speed.

---

## Phase 1: Single-Prompt Spec Generation

Paste into a fresh Claude Code session from the project root:

```
I'm building a new micro-app for my {PROJECT} project called `{APP_NAME}`.

## Project Context
- Monorepo at ~/projects/{PROJECT}/ — see CLAUDE.md for full conventions
- Python 3.12, click CLI, psycopg3, pydantic, structlog, httpx
- All micro-apps: CLI via click, JSON stdout, structured logging to stderr
- Shared types: `from {TYPES_PACKAGE}.models import ...`
- Shared DB: `from {TYPES_PACKAGE}.db import ...`
- Existing packages in packages/ follow a consistent structure (pyproject.toml, Dockerfile, tests/)
- PostgreSQL 15+, all timestamps UTC timezone-aware
- See CLAUDE.md and docs/ for full architecture

## What This App Does
{DESCRIPTION}

## Why It Matters
{BUSINESS_JUSTIFICATION}

## Requirements
{REQUIREMENTS_LIST}

## Deliverables
Following the docs-first workflow:
1. Draft a structured spec at docs/spec-{APP_NAME}.md
2. Run a challenge cycle against it
3. Present challenges for my review
4. After approval, decompose into atomic tasks at docs/{APP_NAME}-tasks.md
5. Output agent execution commands grouped by wave
6. Generate scripts/build-{APP_NAME}.sh — an executable build script using the PROVEN TEMPLATE in docs/micro-app-build-profile.md
```

## Phase 2: Challenge Review

Claude Code outputs challenges (C1, C2, C3...). Review and respond with decisions:

```
Decisions:
- C1: {decision}
- C2: {decision}
- ...
Proceed to approval.
```

## Phase 3: Approval + Decomposition

Claude Code produces:
- `docs/spec-{APP_NAME}.md` — full component spec
- `docs/{APP_NAME}-tasks.md` — atomic tasks with self-contained prompts
- `scripts/build-{APP_NAME}.sh` — executable build script

**IMPORTANT:** All three files must be committed to git BEFORE running the build script.
Agents read task files from the repo; uncommitted files are invisible to them.

## Phase 4: Execution

Run from a regular terminal (NOT from inside Claude Code):

```bash
cd ~/projects/{PROJECT}
bash scripts/build-{APP_NAME}.sh
```

Monitor progress:
```bash
# Check running agents
ps aux | grep claude | grep -v grep

# Watch git log for commits
git log --oneline -10

# If an agent is at 0% CPU for >5 min, it's dead — kill and re-run script
```

---

## Build Script Template (PROVEN — USE THIS EXACTLY)

### Critical Rules (learned the hard way)

1. **ALWAYS include `--dangerously-skip-permissions`** — `claude -p` is non-interactive and cannot prompt for file write permissions.
2. **Parallel tasks in the same wave run in the shared directory** — no worktrees, no branch merges. Wave ends with `git add -A && git commit`.
3. **Parallel tasks must touch different files/directories** — task decomposition guarantees this. If two tasks could edit the same file, they belong in different waves.
4. **Sequential waves commit per-task** — each agent commits before the next starts.
5. **Task files must be committed to git** — agents can only read committed files.
6. **Script must be idempotent** — re-running after interruption should resume from where it left off (agents check existing code).
7. **Always `git push` at the end.**
8. **Always `chmod +x` the script.**

### Template

```bash
#!/usr/bin/env bash
# scripts/build-{APP_NAME}.sh — Automated build via Claude Code agents
set -euo pipefail
ROOT="$(cd "$(dirname "$0")/.." && pwd)"
cd "$ROOT"
TASKS="docs/{APP_NAME}-tasks.md"

# ── Wave 0 (sequential — foundation) ─────────────────────────
echo "── Wave 0: Foundation ──"
claude -p --dangerously-skip-permissions \
  "Read and execute task {PREFIX}-01 from $TASKS. Commit your work when done."

# ── Wave 1 (parallel — independent modules, different dirs) ──
echo "── Wave 1: Modules (parallel) ──"
claude -p --dangerously-skip-permissions \
  "Read and execute task {PREFIX}-02 from $TASKS. Do NOT commit." &
claude -p --dangerously-skip-permissions \
  "Read and execute task {PREFIX}-03 from $TASKS. Do NOT commit." &
claude -p --dangerously-skip-permissions \
  "Read and execute task {PREFIX}-04 from $TASKS. Do NOT commit." &
echo "  waiting for wave 1 agents..."
wait
git add -A && git commit -m "Wave 1: {PREFIX}-02, {PREFIX}-03, {PREFIX}-04"

# ── Wave 2 (sequential — integration) ────────────────────────
echo "── Wave 2: Integration ──"
claude -p --dangerously-skip-permissions \
  "Read and execute task {PREFIX}-05 from $TASKS. Commit your work when done."

# ── Wave 3 (parallel — tests + docker, different dirs) ───────
echo "── Wave 3: Tests + Docker (parallel) ──"
claude -p --dangerously-skip-permissions \
  "Read and execute task {PREFIX}-06 from $TASKS. Do NOT commit." &
claude -p --dangerously-skip-permissions \
  "Read and execute task {PREFIX}-07 from $TASKS. Do NOT commit." &
echo "  waiting for wave 3 agents..."
wait
git add -A && git commit -m "Wave 3: {PREFIX}-06, {PREFIX}-07"

git push
echo "✅ Build complete: {APP_NAME}"
```

---

## Standard Wave Structure

| Wave | Purpose | Execution | Commit Strategy |
|------|---------|-----------|-----------------|
| 0 | Shared type extensions | Sequential | Agent commits |
| 1 | Schema + client + parser | Parallel (different dirs) | Wave commit after `wait` |
| 2 | CLI + wiring | Sequential | Agent commits |
| 3 | Tests + Dockerfile | Parallel (different dirs) | Wave commit after `wait` |

## Parallel Safety Rule

Tasks in the same wave run in the **same working directory** without worktrees.
This is safe because task decomposition guarantees they touch **different files**.

If two tasks could edit the same file, they **must** be in different waves
(sequential). The task decomposition phase enforces this — each task's "Files
to create/modify" list must be disjoint from other tasks in the same wave.

Parallel agents are told "Do NOT commit" — the build script commits once after
all agents in the wave complete. This avoids partial-commit races.

## Variance

Not all micro-apps follow this exact structure:

- **Adapter extensions** (e.g., Alpaca): May include pipeline integration instead of Dockerfile
- **Pure compute apps** (e.g., signal engine): No HTTP client — reads from DB only
- **Multi-system projects** (e.g., trading engine): More waves, more tasks, same pattern
- **Shared library changes**: Wave 0 may touch shared types and require downstream rebuild
- **Fully sequential builds**: Small builds (2-3 tasks) may have no parallel waves at all

The wave count and task count adapt. The pattern is constant:
**spec → challenge → approve → decompose → build script → execute → push**

## Timing Benchmarks

| Component | Tasks | Waves | Est. Time |
|-----------|-------|-------|-----------|
| Full system (Market Watcher) | 13 | 6 | ~4 hours |
| Adapter extension (Alpaca) | 6 | 4 | ~1 hour |
| Enrichment app (Earnings) | 7 | 4 | ~1 hour |
| Signal engine | 7 | 4 | ~1 hour |
| Trading engine | 9 | 4 | ~2 hours |
| CCXT Adapter | 6 | 4 | ~1 hour |
| Bug fixes (TE) | 4 | 3 | ~30 min |
| Schema migrations | 2 | 2 | ~20 min |

## Usage Budget (Max 5x plan)

- Per-task average: ~5-8% of session usage
- Full micro-app build: ~35% of a 5-hour session window
- Multi-system project: may span two session windows
- Check usage: `claude` → `/status`

## Troubleshooting

### Permission errors in non-interactive mode
Ensure `--dangerously-skip-permissions` is on every `claude -p` call.

### Agent seems stuck (>15 min no output)
```bash
ps aux | grep claude | grep -v grep
```
If CPU is 0%, the process is dead. Kill it and re-run the script.
Parallel wave: kill all agents in the wave, then re-run the script.
The script is idempotent — agents check for existing code before writing.

### Task file not found
Task files must be committed to git before running the build script.
```bash
git add docs/{APP_NAME}-tasks.md
git commit -m "Add task decomposition for {APP_NAME}"
```

### Parallel agents edited the same file
This means the task decomposition was wrong — two tasks in the same wave
had overlapping file lists. Fix the task decomposition to move one task
to a later wave. Then `git checkout -- .` and re-run.

### Agent doesn't know about previous wave's code
Parallel agents in the same wave see the state as of the last wave commit.
If an agent needs code from a task in the same wave, those tasks must be
in different waves (sequential dependency).

## Docker Rebuild After Build

After the build script completes, if the system uses Docker:
```bash
docker compose down
docker compose build
docker compose up -d
```

Remember: **code fix → rebuild → restart**. Docker runs the image it built,
not the code on disk. Every code change requires a rebuild.

## New System Bootstrap

For entirely new systems (not micro-apps within existing systems):

```bash
~/bin/init-project.sh {project-name} "{description}"
cd ~/projects/{project-name}
claude
# paste system-level spec prompt
```

This copies architecture docs, creates CLAUDE.md, and initializes git.
Then follow the same spec → challenge → decompose → build cycle.
