---
name: let
description: >-
    Autonomous UK rental property intelligence workflow powered by the `let` CLI toolbelt.
    Use this skill to discover Rightmove listings, gather evidence bundles, verify
    claims, assess top candidates, produce shortlists or region comparisons, and
    coordinate viewing admin when the user explicitly allows email or calendar access.
argument-hint: [search request or location]
compatibility: >-
    Designed for Claude Code with Bash access. Requires an already-installed CLI
    binary at $HOME/.tools/let/let. Network access is needed for live property
    discovery; optional EPC, Mapbox, and source databases improve evidence quality.
allowed-tools: Bash Read Write WebSearch WebFetch
---

# Let

Use `let` as an agent-native property-search toolbelt. The CLI owns discovery,
evidence capture, verification, corrections, persistence, and runtime health.
The agent owns judgment: assess tradeoffs, explain confidence, and decide what
is worth viewing.

## Core Contract

Invoke the installed binary:

```bash
"$HOME/.tools/let/let" <command>
```

Rules:

- If the binary is missing or not executable, report a blocked prerequisite.
- Start from `tools`, `health`, and `config show`; use `tools` again whenever exact flags or output fields are unclear.
- Structured commands emit one JSON envelope on stdout by default. Treat stderr as logs/progress.
- Use `--toon` only when an agent specifically benefits from Toon; it represents the same envelope.
- Bare `let` prints help and is not a JSON command.
- Do not run repo build commands from this skill. Use the installed tool.

## Runtime Context

Read these when they matter to the task:

- `$HOME/.tools/let/data/let.context.md`: human context, priorities, tradeoffs, and assessment lens.
- `$HOME/.tools/let/data/let.config.toml`: baseline search config.
- `$HOME/.tools/let/data/.env`: optional API credentials.
- `$HOME/.tools/let/data/let.db`: local intelligence database.
- `$HOME/.tools/let/sources/`: optional enrichment databases.

Default stance:

- Treat config as the baseline; use command-line overrides for ad-hoc searches.
- Do not edit config or context unless the user asks.
- Record meaningful overrides and evidence gaps in the final report.
- Continue in degraded mode when optional sources, credentials, or media are missing.

## Orientation

Run at the start of a meaningful session:

```bash
"$HOME/.tools/let/let" tools
"$HOME/.tools/let/let" health
"$HOME/.tools/let/let" config show
```

Interpret health:

- `ready`: full local workflow is available.
- `degraded`: continue, but lower confidence where missing evidence matters.
- `blocked`: follow the machine-readable `checks[].fix` guidance, then re-run health.

If the intelligence DB schema is incompatible and the user has no important local data to preserve, the normal recovery is recreating the DB through a fresh inspect flow rather than hand-migrating runtime files.

## Search Workflow

Use small loops:

1. Discover listing IDs.
2. Inspect a small candidate batch.
3. Read evidence and verify checkable claims.
4. Assess the best candidates.
5. Repeat with narrower criteria or a different area.

Discovery guidance:

- Use saved config for baseline searches.
- Use `search resolve <place>` before ad-hoc location searches when a Rightmove location ID is needed.
- Prefer explicit overrides over config edits for one-off searches.
- Watch discovery metadata such as requested limits, truncation, fallback mode, and per-location counts before assuming coverage is complete.
- Treat discovered listing IDs as Rightmove portal IDs.

Inspection guidance:

- Use quick inspection for fast triage, standard inspection for normal evidence, and deeper inspection when media, EPC, verification, or maps materially affect the decision.
- Inspect 2-5 promising listings at a time rather than building a huge stale batch. `inspect` accepts multiple ids/URLs, and when no id is provided it reads ids/URLs from piped stdin automatically.
- Removed listings, rate limits, missing media, and upstream parse drift are normal; skip or retry once, then continue degraded.

Verification guidance:

- Use `evidence <id>` to read the stored bundle. `evidence` also accepts multiple ids or piped stdin for mechanical comparison reads.
- Treat bundle `flags[]` as evidence-quality warnings, not final judgment. They highlight missing evidence, source conflicts, degraded sources, suspicious values, or manual checks to remember.
- When media is inspected and photos are cached, use `media.contactSheet.localPath` for fast visual review before opening individual photos.
- Use `verify <id> --claim <type>` for checkable claims such as broadband, EPC, address, media, or description.
- Distinguish `supported`, `contradicted`, `unknown`, and `insufficientEvidence`; do not collapse them into a boolean.
- If a listing description claims something important, verify it before relying on it.

## Corrections

Use corrections only when a source is wrong and there is better evidence. Corrections are append-only evidence records; they do not rewrite Rightmove snapshots.

Common uses:

- `correct address`: wrong postcode, vague address, incorrect pin, or multiple-flat ambiguity.
- `correct epc`: anchored EPC evidence using certificate URL, LMK key, or UPRN; rating and floor area are extra fields, not enough on their own.
- `correct media`: corrected map coordinates for regenerated map media.
- `correct clear`: disable an active correction without deleting the audit trail.

After any correction:

- Run the returned `nextCommands` or refresh the affected evidence sections.
- Re-read `evidence <id>` and confirm the correction appears before using it in conclusions.

## Assessments

Save assessments as JSON objects with `assess save`; read them with `assess get`.

Assessment guidance:

- The CLI persists agent-authored assessment JSON; it does not enforce a built-in recommendation rubric.
- Prefer the recommended comparison fields when saving assessments: `recommendation`, `confidence`, `summary`, `positives`, `risks`, `nextActions`, `tradeoffs`, `areaNotes`, `commuteNotes`, `familyFit`, `evidenceGaps`, and `source`.
- Recommended `recommendation` values are `view`, `stretch_view`, `backup_view`, `watch`, `pass`, and `benchmark`; recommended `confidence` values are `high`, `medium_high`, `medium`, and `low`.
- The raw assessment remains flexible, but `assess get`, `assess list`, and evidence bundles expose `normalizedAssessment` for comparison.
- Keep listing quality and location quality separate.
- Lower confidence when key evidence is missing instead of inventing values.
- Use deterministic scores only as background context unless the user explicitly asks for scoring experiments.

Evaluate the factors that matter for the user's stated context, such as:

- maintenance, damp, and poor renovation signals
- light, layout, storage, proportions, and missing rooms
- EPC, heating, floor area, and running-cost clues
- broadband, transport, safety, schools, amenities, flood risk, and neighborhood feel
- dealbreakers and tradeoffs from the context file or the current user request

## Reporting

Return concise, evidence-grounded results:

- What was searched or inspected, including notable overrides and freshness.
- Top candidates with links, fit rationale, red flags, and evidence status.
- Region comparisons when multiple areas are in scope.
- Clear next steps: inspect more, verify a claim, correct bad evidence, request a viewing, or adjust config.
- Missing evidence and confidence limits.

Prefer compact tables for shortlist comparison, then short notes for non-obvious positives or risks.

## Admin Work

Use Gmail or Calendar tools only when the user explicitly asks for or clearly permits viewing admin.

Rules:

- Verify the property identity before using inbox or calendar context.
- Search email by portal ID, address fragments, and agent names when matching confirmations.
- Ask which calendar to use unless the user has specified one or the convention is already clear from nearby events.
- Create or update calendar events only after booking details are confirmed.
- Re-read the target day after calendar writes to check for duplicates or obvious conflicts.
- Do not hardcode account names, calendar IDs, or private naming conventions in this skill.

## Error Handling

When a command fails, read `error.code`, `error.message`, and `error.hint`.

General recovery:

- Missing config: create or repair config only with user approval unless setup was explicitly requested.
- Missing sources or credentials: continue degraded unless the task depends on that evidence.
- Schema mismatch: recreate runtime DB when acceptable; avoid hand-editing DB files.
- Lock conflict: close competing DB users and retry.
- Network or parse errors: retry once, then continue degraded and report the gap.
- Validation errors: fix inputs according to the hint and retry.

Exit codes:

- `0`: success, including documented partial success.
- `1`: runtime or validation failure.
- `2`: blocked prerequisite.

## Parallelism

- Search and inspect work can contend on the same DB and cache; keep location exploration sequential unless you know the work is isolated.
- Assessment-only work may run in parallel when each worker owns disjoint listing IDs.
- Every subagent should use `tools`, `health`, evidence reads, and verification rather than guessing command shape or scraping text.
