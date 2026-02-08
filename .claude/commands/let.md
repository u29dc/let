# Property Search Agent

Autonomous property search pipeline using the `let` CLI toolbelt. Discovers listings, fetches selectively, triages by score, deep-dives top candidates with photo/map analysis, writes assessments, and produces a final report.

## Prerequisites

Run from the repository root. The CLI binary should be built (`bun run build:cli`) or use `bun run let` directly.

## 1. Orient

```bash
let tools --json               # discover available commands
let health --json              # check prerequisites
let config show --json         # understand locations, scoring weights, filters
```

If health returns `status: "blocked"`: execute fix commands from `checks[].fix`, re-check. If `status: "degraded"`: proceed with partial enrichment (scoring works with lower confidence).

## 2. Discover

```bash
let search discover --json                     # find portal IDs across all configured locations
let search diff <comma-separated-ids> --json   # classify new vs known
```

Use IDs from discover step. The diff command tells you which are new vs already in the database.

## 3. Acquire

```bash
let fetch <new-ids> --json                     # fetch in batches of 10-15
```

Check `failed[]` in response -- retry once for transient errors, skip permanent (404). Each fetch includes parse, enrich (EPC, broadband, area metrics), and re-score of entire database.

## 4. Triage

```bash
let view list --top 30 --json                  # ranked overview
```

Classify by algorithm score:
- **>= 80**: Must assess (high priority)
- **65-79**: Assess if time permits
- **< 65**: Skip unless specific interest

## 5. Assess

```bash
let assess candidates --json                   # queue of unassessed listings
let assess context <id> --json                 # get everything needed for assessment
```

### Batch Assessment via Parallel Subagents

For efficiency, partition candidate IDs across 5-10 subagents (2-3 listings each) using the Task tool with `subagent_type=general-purpose`. Each subagent gets disjoint IDs; writes are per-listing atomic.

**Subagent prompt template** (replace `{IDS}` with comma-separated listing IDs):

```
Assess listings: {IDS}

For each ID:
1. Run `let assess context {id} --json` to get listing details, score breakdown, and media paths
2. Glob `.cache/{id}/*.webp` then Read each image (property photos, satellite, street map)
3. Analyze: maintenance quality, natural light, spaciousness, what photos show/hide, neighborhood from maps
4. Submit: `let assess submit {id} --data '{"maintenance":"...","lightAndSpace":"...","photoAnalysis":"...","neighborhoodAnalysis":"...","recommendation":"...","familySuitability":"...","reasoning":"...","scoreAdjustment":0}' --json`

Assessment guidance:

MAINTENANCE (from photos): excellent (pristine, renovated) / good (clean, no issues) / fair (dated but functional) / poor (neglected, damage)
LIGHT/SPACE: window size, brightness, ceiling height, room proportions, layout flow, south-facing
PHOTOS: missing rooms = red flag, awkward angles, wide-angle distortion, dark/edited, damage signs
NEIGHBORHOOD (maps show ~10min walk radius with red pin): green space, density, industrial concerns, busy roads, walkability
FAMILY: safe play areas, storage, school/park proximity, quiet indicators
RECOMMENDATION: strong-recommend / recommend / neutral / avoid
SCORE ADJUSTMENT (-30 to +30): positive = better than algorithm suggests, negative = worse, 0 = no change

No coordination needed - your IDs are unique.
```

### Image Cache

| Aspect | Detail |
| ------ | ------ |
| Location | `.cache/{id}/` |
| Photos | `{id}-photo-{hash}.webp` |
| Maps | `{id}-satellite-{hash}.webp`, `{id}-street-{hash}.webp` |
| Format | WebP 900-1200px, maps show ~10min walk radius |

## 6. Report

```bash
let view list --top 20 --json                  # final rankings with assessed scores
let view detail <id> --json                    # full property data for top picks
let score explain <id> --json                  # score breakdown
```

### Report Structure

**Overview**: total listings, regions, score distribution (80+/60-79/40-59/<40), price range, bedroom split, assessment coverage.

**Top Properties** (table, top 10):

| Rank | Property | Region | Price | Beds | Size | EPC | Garden | Algo | Assessed | AI Rec | Notes |
| ---- | -------- | ------ | ----- | ---- | ---- | --- | ------ | ---- | -------- | ------ | ----- |

- Property: `[Address](https://www.rightmove.co.uk/properties/{id})`
- AI Rec: SR (strong-recommend) / R (recommend) / N (neutral) / A (avoid) / - (not assessed)

**Region Comparison** (table):

| Region | Count | Avg Score | Best Value | Verdict |
| ------ | ----- | --------- | ---------- | ------- |

**Deep Dives** (top 3-5):

```
**#1: [Address](link)** - Price/mo, X bed, Xsqm
- EPC X | Broadband X% | X.Xmi to [Station]
- Score: Algo X → Assessed X (adjustment +/-Y)
- AI: [maintenance], [recommendation], [key reasoning]
- Why it ranks: [1 sentence]
```

**Recommendations**: must-view (top 3 with links), watch list, needs assessment, search refinements.

## Error Recovery

| Error Code | Action |
| ---------- | ------ |
| `NO_CONFIG` | Tell user to create config from template. Cannot proceed. |
| `NO_SOURCES` | Tell user: run `fix[]` commands. Continue without -- lower confidence. |
| `NO_DATABASE` | Normal on first run. Proceed to discover + fetch. |
| `RATE_LIMITED` | Wait 60s, retry with `--delay 5000`. |
| `NOT_FOUND` | Listing removed from portal. Skip and continue. |
| `VALIDATION_ERROR` | Fix assessment JSON (check enums, string lengths, scoreAdjustment range). |
| `API_ERROR` | Log, skip affected enrichment, continue with available data. |

## Score Interpretation

| Range | Meaning |
| ----- | ------- |
| 85-100 | Exceptional -- strong across all dimensions |
| 70-84 | Good -- strong in most areas, minor weaknesses |
| 55-69 | Average -- mixed, moderate penalties |
| 40-54 | Below average -- significant weaknesses |
| < 40 | Poor -- major penalties |

Scores are percentile-relative within the current database. The agent adds value by detecting what photos reveal, researching neighborhoods, and identifying tradeoffs the algorithm cannot weigh.
