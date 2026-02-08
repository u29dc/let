# Operating Protocol

Full pipeline protocol for the `let` CLI. Follow these phases in order. All commands use `--json` for structured output; read stdout as the contract, ignore stderr (logs only).

> All examples use `bin/let` -- the compiled binary at the repo root. Build with `bun run build:cli` if missing.

## Core principles

- **Config is the baseline**: use it as the default preference set.
- **Overrides are explicit**: for ad-hoc requests (new city, flats vs houses, relaxing garden), prefer one-off CLI flags rather than editing config. Always report overrides in the final output and do not persist them unless asked.
- **Scores are advisory**: use algorithmic scores for triage; apply judgment based on photos, layout, and neighborhood research.
- **Deterministic contracts**: every `--json` command outputs exactly one JSON envelope to stdout.

## Phase 0: Orient (always)

```bash
bin/let tools --json
bin/let health --json
bin/let config show --json
```

If `health.status == "blocked"`: execute fix commands from `checks[].fix`, then re-run health.
If `health.status == "degraded"`: proceed (enrichment/scoring still works, but confidence may be lower).

## Phase 1: Define mode (baseline vs override)

### Baseline mode (default)

Use config locations and filters as-is.

### Override mode (ad-hoc)

Used for prompts like:

- "check flats in Manchester"
- "compare Manchester, Liverpool, Sheffield"
- "flats around York within ~30 min of York city centre"

Rules:

- Prefer CLI overrides. Do not edit config unless there is no tool support.
- Always record in your final report:
    - `Overrides applied:` (bullet list)
    - `What stayed from config:` (short bullet list)

Available override flags for `search discover`:

| Flag                       | Example                           | Effect                                                      |
| -------------------------- | --------------------------------- | ----------------------------------------------------------- |
| `--location <ID>`          | `--location REGION^904`           | Search a non-config location (get ID from `search resolve`) |
| `--location-name <name>`   | `--location-name Manchester`      | Display name for ad-hoc location                            |
| `--property-types <list>`  | `--property-types flat,apartment` | Override property types                                     |
| `--must-have <list\|none>` | `--must-have none`                | Override mustHave filters ("none" clears)                   |
| `--dont-show <list\|none>` | `--dont-show none`                | Override dontShow filters ("none" clears)                   |
| `--limit <n>`              | `--limit 50`                      | Max results per location                                    |

When `--location` is used, `mustHave`, `dontShow`, and `propertyTypes` are automatically cleared unless explicitly passed. Ad-hoc searches start from a blank slate. To carry forward a config filter, pass it explicitly (e.g., `--must-have garden`).

For fetch: `--region <name>` assigns region to fetched listings (use displayName from `search resolve`).

## Phase 2: Discover

Definition: **"new listings" = portal IDs not present in the SQLite DB yet**.

```bash
bin/let search discover --json
bin/let search diff <comma-separated-ids> --json
```

The `discover` output includes `idsByLocation` -- a map of location name to portal IDs. Use this to batch `fetch` calls by region:

```bash
bin/let search discover --json
# Output includes: idsByLocation: { "Sheffield": ["id1", ...], "Stamford": ["id2", ...] }
bin/let fetch <sheffield-ids> --region Sheffield --json
bin/let fetch <stamford-ids> --region Stamford --json
```

Guidance:

- If DB is empty, `diff.new` may be "everything." Start by fetching a small sample first (5-10) to calibrate.
- Prefer repeated small loops over one giant run: discover, diff, fetch, triage, repeat.

## Phase 3: Acquire (fetch + enrich + score + persist)

```bash
bin/let fetch <new-ids> --json
```

Batching guidance:

- Start with batches of **5-10** IDs for fast feedback, then increase to **10-15** once stable.
- If you see rate limiting, increase delay and retry once (do not spam).
- Treat missing/removed listings as normal; skip after one retry if clearly permanent.

Failure handling:

- If some IDs fail, continue the run.
- If media is missing (images skipped or not cached), mark the assessment as lower confidence or re-fetch just the top 1-2 without skipping media.

## Phase 4: Triage (ranked overview)

```bash
bin/let view list --top 30 --json
```

Suggested triage tiers (algorithm score):

- **>= 80**: must assess
- **65-79**: assess if time permits
- **< 65**: skip unless a specific feature is compelling

Prefer to assess a smaller number deeply (2-5) rather than shallowly reviewing 30.

## Phase 5: Assess (deep dive + write judgment back)

Queue and context:

```bash
bin/let assess candidates --json
bin/let assess context <id> --json
```

How to use assessment context correctly:

- Do **not** guess cache paths.
- Use the `media.*` paths returned by `assess context` to locate images/maps/floorplans (those paths are the source of truth).
- Use the included assessment schema as your contract for submission fields.

What to look for:

- Maintenance (condition, damp indicators, DIY quality)
- Light and space (windows, layout flow, ceiling height, storage)
- Missing photos/floorplan (red flags)
- Neighborhood via maps (busy roads, green space, density, industrial sites)
- Quick web research where needed (schools, safety, amenities, transport)

Submit assessment:

```bash
bin/let assess submit <id> '<assessment-json>' --json
```

Assessment rules:

- Keep it evidence-based.
- If you adjust the score, explain why in 1-2 sentences.
- If media was missing, state that and lower confidence.

### Batch assessment via parallel subagents (optional)

For speed, split candidate IDs across 5-10 subagents (2-3 listings each). Each subagent gets disjoint IDs; assessment writes are per-listing atomic (targeted SQL UPDATE, not full DB rewrite). Parallel subagents submitting to different listings will not conflict.

Subagent prompt template (replace `{IDS}`):

```
Assess listings: {IDS}

For each listing ID:
1) Run `bin/let assess context {id} --json`
2) Use the returned `media` paths (do not guess cache directories)
3) Review images/maps; do a quick neighborhood web check if needed
4) Submit: `bin/let assess submit {id} '<valid assessment JSON>' --json`

Guidance:
* Maintenance: excellent / good / fair / poor
* Light/space: brightness, layout, proportions
* Photos: missing rooms = red flag
* Neighborhood: busy roads, green space, walkability
* Recommendation: strong-recommend / recommend / neutral / avoid
* Score adjustment: -30 to +30
```

## Phase 6: Report (shortlist + comparison + next steps)

```bash
bin/let view list --top 20 --json
bin/let view detail <id> --json
bin/let score explain <id> --json
```

Report structure:

- **Overview**: count of new fetched, assessed coverage, score distribution, price range.
- **Top picks (3-5)**: for each:
    - Link
    - Algorithm vs assessed score (+ explanation if adjusted)
    - Key positives/negatives
    - Neighborhood notes
    - Clear next steps (viewing, questions to ask, watchlist)
- **If comparing regions**:
    - Sample size per region
    - Average score and "best value" examples
    - A short verdict per region (fit vs tradeoffs)

## Common intent mappings

- **"Top 5 new homes"**: Baseline mode. Discover, diff, fetch new (small batches), triage, assess top 2-3, report top 5.

- **"Compare Manchester, Liverpool, Sheffield"**: Override mode. For each city: resolve, discover, fetch small sample, triage, produce region summary, compare.

- **"Flats around York, ~30 min drive to city centre"**: Override mode (property type = flats). Use available location tools to discover surrounding towns and compare. If no travel-time primitive exists, approximate and label it as an approximation.

## Error recovery

| Error Code         | Action                                                                                      |
| ------------------ | ------------------------------------------------------------------------------------------- |
| `NO_CONFIG`        | Create config from template. Cannot proceed without it.                                     |
| `NO_SOURCES`       | Run `fix[]` commands if you want full enrichment; you may proceed with degraded confidence. |
| `NO_DATABASE`      | Normal on first run. Proceed to fetch; DB will be created.                                  |
| `SCHEMA_MISMATCH`  | Delete the database and re-fetch. Schema has changed incompatibly.                          |
| `RATE_LIMITED`     | Wait, increase delay, retry once.                                                           |
| `NOT_FOUND`        | Listing removed from portal. Skip and continue.                                             |
| `VALIDATION_ERROR` | Fix assessment JSON according to schema.                                                    |
| `API_ERROR`        | Log, skip affected enrichment, continue with available data.                                |
