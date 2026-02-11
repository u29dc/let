---
name: let
description: >-
    Autonomous UK rental property search workflow powered by the `let` CLI toolbelt.
    Use this skill to discover Rightmove listings, enrich and score them, assess top
    candidates (photos/maps + neighborhood research), and produce shortlists and
    region comparisons for a family's preferences.
compatibility: >-
    Designed for Claude Code with Bash access. Requires `:let` to be built
    (bun run build:cli). Network access for Rightmove; optional EPC/Mapbox/Notion
    keys enable richer enrichment and exports.
allowed-tools: Bash Read Write WebSearch WebFetch
---

## Invocation

`:let` is a shell alias for the compiled binary. Use it directly in bash:

    :let <command>

NEVER use `bun run let` in agent workflows -- that is the dev entrypoint.

If `:let` is not found, build it first: `bun run build:cli` (in the repo root).

## Orientation

> If `:let` is not found, run `bun run build:cli`.

1. Read `$LET_HOME/data/let.context.md` first (human context for the config: the user's situation, the current benchmark, what "100/100" means, and which tradeoffs are acceptable).

2. Then run the base checks:

- `:let tools --json`
- `:let health --json`
- `:let config show --json`

If health is blocked, follow the fix guidance in `references/init.md`.

## Config vs context

- If a config TOML exists and loads successfully, treat it as the baseline for searches.
- The config should roughly reflect the preferences in `.let/data/let.context.md`, but it may drift over time.
    - If you notice mismatches (e.g., "must-have garden" missing, budget out of date, regions missing), flag them clearly in your final report.
    - After the run, recommend specific config updates (do not edit config unless explicitly asked).
- Depending on how "relaxed" the user's request is, you may run some ad-hoc exploration using one-off CLI overrides (e.g., try an extra region/city, switch flats vs houses) without changing the saved config. Always report what you overrode.

## Subagent execution

Multi-region searches can be context-token heavy. Use subagents to keep each location's work contained. Use subagents sequentially, not in parallel -- parallel subagents can conflict when writing to the same SQLite DB and cache.

1. Main agent does orientation and sets the plan (locations to explore, batch sizes, "strict vs relaxed" mode).
2. For each location, spawn one subagent, wait for completion, then spawn the next location's subagent.

## Subagent template

When delegating a location to a subagent, give it:

- The user context summary (from `.let/data/let.context.md`)
- The user's current request (what you're trying to achieve)
- The location to explore (name + identifier if known)
- The rule: do not edit config; use overrides if needed; write assessments back; keep work small

Subagent prompt template (replace `{LOCATION}` / `{LOCATION_ID}`):

```
You are a subagent exploring one location for the `let` property search.

Read this first: .let/data/let.context.md (family context + preferences).

Constraints:
* Do not edit config files.
* Use `--json` for tool calls.
* Keep the batch small (discover, then fetch 5–10 max).
* Assess 1–2 best candidates deeply if media is available.
* Write assessments back using the normal assessment submission flow.
* Return a compact summary for this location.

Location:
* Name: {LOCATION}
* Identifier (if available): {LOCATION_ID}

Steps (use the tool catalog to confirm signatures):
1) Orient quickly: `:let health --json` (ensure not blocked)
2) Discover listings for this location (baseline or override mode as appropriate)
3) Diff new vs known
4) Fetch a small batch (5–10), assign region name if relevant
5) Triage (top 10 list)
6) Deep dive 1–2: context + photos/maps + quick neighborhood research
7) Submit 1–2 assessments
8) Return:
    * Top 3 candidates (links + 1–2 sentence rationale each)
    * Any red flags (crime/deprivation/obvious neighborhood issues, missing media, etc.)
    * Any overrides used (region/property type/must-have changes)
    * "Is this location a good fit for our 'Bath-like but affordable' goal?" (short verdict)
```

## Self-describing CLI

Run `:let tools --json` whenever you're uncertain about parameters or command signatures. Treat it as the source of truth.

## On-demand references

- Full end-to-end procedure and command patterns: `references/protocol.md`
- Setup and first-run remediation: `references/init.md`
- Score interpretation and search override guidance: `references/scoring.md`
- Error codes and recovery actions: `references/errors.md`

## Expected output

When the user asks you to "search," you should return:

- A region-by-region comparison (fit + value + tradeoffs)
- A final shortlist (top 3-5) with links and clear rationale
- Any suggested config refinements (after the run)
- A brief list of what you actually did (so the user trusts the process)
