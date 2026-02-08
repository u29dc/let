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

## 5. Assess (for each top candidate)

```bash
let assess candidates --json                   # queue of unassessed listings
let assess context <id> --json                 # get everything needed for assessment
```

Then for each candidate:
1. Read images from `media.images` paths (Glob + Read the image files)
2. Review satellite/street map images from `media.mapViews`
3. Check `scoreBreakdown` for algorithmic reasoning
4. Web search the neighborhood if needed

Submit assessment:
```bash
let assess submit <id> --data '{"maintenance":"good","lightAndSpace":"bright rooms, good ceiling heights","photoAnalysis":"all rooms shown, honest angles","recommendation":"recommend","familySuitability":"good","reasoning":"well-maintained with good transport links","scoreAdjustment":5}' --json
```

### Assessment Field Guide

**maintenance** (`excellent` / `good` / `fair` / `poor`):
- excellent: pristine, recently renovated, quality finishes
- good: well-maintained, clean, no visible issues
- fair: dated but functional, minor wear
- poor: neglected, visible damage, needs significant work

**lightAndSpace**: Window size, brightness, ceiling height, room proportions, layout flow, south-facing orientation.

**photoAnalysis**: Coverage quality -- missing rooms are a red flag. Note awkward angles, excessive wide-angle, dark/edited photos, visible damage or damp.

**neighborhoodAnalysis** (optional): Findings from satellite + street maps. Green space, density, industrial concerns, busy roads, walkability.

**recommendation** (`strong-recommend` / `recommend` / `neutral` / `avoid`):
- strong-recommend: exceptional, all criteria met
- recommend: good with minor compromises
- neutral: average, nothing stands out
- avoid: significant issues identified

**familySuitability** (`excellent` / `good` / `fair` / `poor`): Safe play areas, storage, school/park proximity, quiet indicators.

**reasoning**: Explain the recommendation. Reference specific observations.

**scoreAdjustment** (-30 to +30): Manual score adjustment. Use 0 for no change. Positive for properties better than algorithm suggests, negative for worse.

**tradeoffs** (optional): Compensating factors like "north garden but conservatory floods with light".

## 6. Report

```bash
let view list --top 20 --json                  # final rankings with assessed scores
```

Present top 3-5 with: Rightmove links, score breakdowns (algo vs assessed), assessment summaries, neighborhood context, tradeoffs, and recommended next steps.

## Error Recovery

| Error Code         | Action                                                                     |
| ------------------ | -------------------------------------------------------------------------- |
| `NO_CONFIG`        | Tell user to create config from template. Cannot proceed.                  |
| `NO_SOURCES`       | Tell user: run `fix[]` commands. Continue without -- lower confidence.     |
| `NO_DATABASE`      | Normal on first run. Proceed to discover + fetch.                          |
| `SCHEMA_MISMATCH`  | Tell user: backup exists. May need to recreate DB.                         |
| `RATE_LIMITED`     | Wait 60s, retry with `--delay 5000`.                                       |
| `NOT_FOUND`        | Listing removed from portal. Skip and continue.                           |
| `VALIDATION_ERROR` | Fix assessment JSON (check enums, string lengths, scoreAdjustment range). |
| `API_ERROR`        | Log, skip affected enrichment, continue with available data.              |

## Score Interpretation

| Range  | Meaning                                        |
| ------ | ---------------------------------------------- |
| 85-100 | Exceptional -- strong across all dimensions    |
| 70-84  | Good -- strong in most areas, minor weaknesses |
| 55-69  | Average -- mixed, moderate penalties           |
| 40-54  | Below average -- significant weaknesses        |
| < 40   | Poor -- major penalties                        |

Scores are percentile-relative within the current database. The agent adds value by detecting what photos reveal, researching neighborhoods, and identifying tradeoffs the algorithm cannot weigh.
