---
name: let
description: >-
    Autonomous UK rental property intelligence workflow powered by the `let` CLI toolbelt.
    Use this skill to discover Rightmove listings, gather evidence bundles, verify
    claims, assess top candidates (photos/maps + neighborhood research), produce shortlists and
    region comparisons for a family's preferences, and coordinate viewing admin
    when the user explicitly allows email or calendar access.
argument-hint: [search request or location]
compatibility: >-
    Designed for Claude Code with Bash access. Requires an already-installed
    CLI binary at $HOME/.tools/let/let. Network
    access for Rightmove; optional EPC and Mapbox keys enable richer evidence.
allowed-tools: Bash Read Write WebSearch WebFetch
---

# Let

Autonomous UK rental property discovery, evidence gathering, verification, and neighborhood assessment via the `let` CLI.

## How to Use

- Use when the user wants new rental listings, a region comparison, or a deeper review of shortlisted homes.
- Use when the user wants an existing `let` setup initialized, repaired, or explained.
- Use when the user wants listing assessments written back into the local `let` intelligence database.
- Use when the user wants viewing admin tied to a listing, such as checking confirmation emails or creating calendar events.

## Invocation

```bash
"$HOME/.tools/let/let" <command>
```

Hard rules:

- If `"$HOME/.tools/let/let"` is missing or not executable, return a blocked prerequisite and stop.
- All non-interactive commands emit JSON envelopes on stdout by default. Use `--toon` only when Toon is explicitly useful; treat stderr as logs/progress.
- Read stdout as the contract. Treat stderr as logs only.
- Bare `"$HOME/.tools/let/let"` prints clap help; it does not emit JSON.
- Do not run repo build commands from this skill.

## Operating Rules

- Read `$HOME/.tools/let/data/let.context.md` first. It contains the human context behind the config.
- Treat config as the baseline. Use one-off CLI overrides for ad-hoc searches; do not edit config unless explicitly asked.
- Record every override in the final report.
- Treat deterministic scores as optional context only. The AI agent owns the final assessment.
- Use repeated small loops: discover ids, inspect selected listings, read evidence, verify claims, assess, repeat.
- Continue on partial failures. Missing listings, missing media, or missing enrichment are normal.
- Never guess cache paths. Use URLs and local paths returned in evidence bundles.
- Search/inspect subagents run sequentially by location when they can contend on the same DB and cache.
- Assessment-only subagents may run in parallel only when each subagent owns disjoint listing IDs.
- Treat inbox and calendar access as opt-in and sensitive. Use `gog` only when the user explicitly asks for or clearly allows email or calendar work.

## Data Files

- `$HOME/.tools/let/data/let.context.md`: prose summary of the family's situation, priorities, tradeoffs, and what "100/100" means.
- `$HOME/.tools/let/data/let.config.toml`: baseline search config.
- `$HOME/.tools/let/data/.env`: EPC, Mapbox, and optional Notion credentials.
- `$HOME/.tools/let/data/let.db`: intelligence DB containing entities, source snapshots, facts, claims, verifications, evidence bundles, and assessments.
- `.claude/skills/let/templates/let.config.toml`: config template for first-run setup.
- `$HOME/.tools/let/sources/`: local enrichment databases for broadband, IMD, crime, flood, census, and income.

## Orientation and Health

Run these at the start of every meaningful session:

```bash
"$HOME/.tools/let/let" tools
"$HOME/.tools/let/let" health
"$HOME/.tools/let/let" config show
```

Interpretation:

- `status: "ready"`: full pipeline available.
- `status: "degraded"`: continue, but report lower confidence where enrichment is missing.
- `status: "blocked"`: apply the provided `checks[].fix` commands, then re-run health.

If context or config drift is obvious:

- Flag the mismatch clearly in the final output.
- Recommend exact config updates after the run.
- Do not edit config unless explicitly asked.

## Setup and Remediation

Use this section when `health` is blocked, the setup is incomplete, or the user's situation has materially changed.

### Context and Config

- If `NO_CONFIG` is reported, create `$HOME/.tools/let/data/let.config.toml` from `.claude/skills/let/templates/let.config.toml`.
- Ask open-ended context questions first, then config-specific questions.
- Write or update `$HOME/.tools/let/data/let.context.md` in natural prose. Do not reduce it to config bullets.

Phase 1 questions capture the human problem:

- What does the perfect home look like?
- What do you love about where you live now?
- What is the one thing you would change?
- Describe the ideal neighborhood in one sentence.
- Who is moving: family size, ages, pets, remote work, commute needs?

Phase 2 questions fill the config:

- Areas under consideration. Resolve each with `"$HOME/.tools/let/let" search resolve <name>`.
- Property types.
- Budget range.
- Bedroom range.
- Garden importance.
- Deal-breakers for `dontShow`.
- Region preference ranking for `[scoring.regionPriority]`.

Config notes:

- Config path: `$HOME/.tools/let/data/let.config.toml`.
- Core sections: `[search]`, `[search.filters]`, `[fetch]`, `[scoring]`, `[scoring.regionPriority]`.
- `search.useApi` controls only Rightmove discovery transport:
  `true` = API first with HTML fallback, `false` = HTML-only discovery.
- `search.useApi` does not affect EPC evidence. EPC API usage is controlled by credentials and the requested evidence sections.
- Region priority scores are `0-100`. Higher values increase ranking preference for that area.

### API Keys

- `EPC_API_BEARER_TOKEN` in `$HOME/.tools/let/data/.env`: preferred for live EPC rating, floor area, and EPC-derived UPRN enrichment on the new Get Energy Performance Data service.
- Legacy transition fallback: `EPC_API_EMAIL` and `EPC_API_KEY` still work only while Open Data Communities remains available.
- `MAPBOX_ACCESS_TOKEN`: optional, enables external address geocoding.
- `NOTION_API_KEY` and `NOTION_DATABASE_ID`: optional and not part of the current core assessment loop.
- If credentials are missing, continue unless the requested task specifically depends on them.

### Source Databases

- If source databases are missing, the pipeline still works in degraded mode.
- Degraded mode means weaker location confidence: no broadband, IMD, crime, flood, census, or income enrichment.
- To build sources locally:

```bash
"$HOME/.tools/let/let" sources build all --jobs 3
```

- Optional integrity guard: set matching `*_SHA256` env vars (for example `POSTCODES_ZIP_SHA256`) before running source builds to enforce SHA-256 verification.
- Expect roughly 5-10GB of downloads and 10-30 minutes of runtime.

### Final Verification

After setup or remediation, run:

```bash
"$HOME/.tools/let/let" health
```

Proceed only when status is `ready` or `degraded`.

## Search Modes

### Baseline Mode

Use saved config locations and filters as-is.

### Override Mode

Use for ad-hoc prompts such as:

- check flats in Manchester
- compare Manchester, Liverpool, Sheffield
- flats around York within about 30 minutes of the city centre

Use CLI overrides instead of editing config. Always report:

- `Overrides applied`
- `What stayed from config`

Override flags for `search discover`:

| Flag | Example | Effect |
| --- | --- | --- | --- |
| `--location <ID>` | `--location REGION^904` | Search a non-config location |
| `--location-name <name>` | `--location-name Manchester` | Display name for ad-hoc location |
| `--property-types <list>` | `--property-types flat,apartment` | Override property types |
| `--must-have <list\ | none>` | `--must-have garden` | Override must-have filters |
| `--dont-show <list\ | none>` | `--dont-show houseShare,student` | Override excluded listing types |
| `--limit <n>` | `--limit 50` | Max results per location |

Rules:

- When `--location` is used, `mustHave`, `dontShow`, and `propertyTypes` are cleared unless explicitly re-passed.
- Ad-hoc location searches start from a blank slate. Carry forward desired filters explicitly.
- When fetching an ad-hoc location batch, use `--region <name>` to stamp the display region.
- `sourceMode: "html"` means config-driven HTML-only discovery; `sourceMode: "html-fallback"` means the API was attempted first and HTML was used after fallback.
- Read `requestedLimit`, `effectivePageSize`, `pagesFetched`, `truncated`, and per-location `locations[]` stats before assuming discovery is complete.

## Workflow

Follow these phases in order.

### Phase 0: Orient

```bash
"$HOME/.tools/let/let" tools
"$HOME/.tools/let/let" health
"$HOME/.tools/let/let" config show
```

Run `"$HOME/.tools/let/let" tools` again whenever command shape or parameters are unclear.

### Phase 1: Discover

```bash
"$HOME/.tools/let/let" search discover
```

Rules:

- Treat discovered listings as Rightmove portal IDs.
- Use `idsByLocation` from `search discover` to choose focused inspection batches.
- If `truncated` is true or a location reports a `truncationReason`, treat that region as partial coverage and say so.
- Prefer repeated small loops over one giant run.

### Phase 2: Inspect

```bash
"$HOME/.tools/let/let" inspect <rightmove-id> --depth standard
"$HOME/.tools/let/let" inspect <rightmove-id> --depth deep --section rightmove,address,broadband,epc,media,verifications
```

Rules:

- Start with 2-5 promising IDs.
- Use `--depth quick` for fast Rightmove-only triage, `standard` for normal evidence, and `deep` when EPC or extra verification is warranted.
- If rate limited, increase delay and retry once. Do not spam.
- Treat removed listings as normal; skip after one clear failure.
- Missing source DBs, EPC credentials, or Mapbox token should degrade only the relevant evidence sections.

### Phase 3: Read and Verify

```bash
"$HOME/.tools/let/let" evidence <rightmove-id>
"$HOME/.tools/let/let" verify <rightmove-id> --claim broadband
```

Rules:

- Use `sections` to understand confidence and missing evidence.
- If a description mentions gigabit or full fibre, read broadband verification before treating the claim as true.
- Treat `supported`, `contradicted`, `unknown`, and `insufficientEvidence` distinctly.
- Prefer 2-5 deep assessments over 30 shallow reviews.

### Phase 3A: Correct Known Source Errors

Use corrections when Rightmove or another source is wrong and the user or agent has better evidence.

```bash
"$HOME/.tools/let/let" correct address <rightmove-id> --postcode '<postcode>' --note '<why this is trusted>'
"$HOME/.tools/let/let" correct address <rightmove-id> --address '<exact address>' --lat <lat> --lng <lng> --note '<why this is trusted>'
"$HOME/.tools/let/let" correct epc <rightmove-id> --lmk-key '<key>' --rating C --note '<matched EPC source>'
"$HOME/.tools/let/let" correct media <rightmove-id> --map-lat <lat> --map-lng <lng> --note '<manual map pin>'
"$HOME/.tools/let/let" correct clear <rightmove-id> --kind address --correction-id <id>
```

Rules:

- Corrections are append-only evidence records. They do not overwrite Rightmove snapshots.
- Use `correct address` for wrong postcodes, vague display addresses, incorrect map pins, or multiple-flat ambiguity when a better address/postcode/coordinate is known.
- Use `correct epc` when the exact certificate URL, LMK key, UPRN, rating, or floor area is known.
- Use `correct media` for map-coordinate corrections that should regenerate map media.
- After any correction, run the suggested `nextCommands` from the correction response or run `inspect <id> --refresh all --section <affected-sections>`.
- Re-read `evidence <id>` and confirm the `corrections` array appears before relying on corrected conclusions.

### Phase 4: Assess

Save the agent's assessment:

```bash
"$HOME/.tools/let/let" assess save <rightmove-id> '<assessment-json>'
"$HOME/.tools/let/let" assess get <rightmove-id>
```

Assessment rules:

- Store a JSON object. The CLI does not enforce a scoring rubric.
- Include recommendation, reasoning, evidence gaps, red flags, and any viewing priority when useful.
- Invalid non-object submissions fail with `VALIDATION_ERROR`.
- Keep conclusions evidence-based.
- If media is missing, say so and lower confidence rather than guessing.

What to evaluate:

- maintenance and signs of damp or poor DIY work
- light, layout, proportions, storage, and flow
- missing rooms or missing floorplans
- neighborhood character from maps and nearby context
- schools, safety, amenities, and transport where relevant to the user's priorities

### Phase 5: Report

Useful commands:

```bash
"$HOME/.tools/let/let" evidence <rightmove-id>
"$HOME/.tools/let/let" verify <rightmove-id> --claim all
"$HOME/.tools/let/let" assess get <rightmove-id>
```

Report structure:

1. One-line overview with sample size, freshness, and price range.
2. One comparison table for the top picks.
3. Numbered notes keyed to table rows with non-obvious positives, negatives, and red flags.
4. Short verdict tied to the user's stated priorities.
5. Clear next steps.

Recommended comparison columns:

```text
| # | Address | pcm | Beds | Type | EPC | Broadband | Crime/1k | IMD | Evidence status | Link |
```

If comparing regions, include:

- sample size per region
- average evidence quality and fit by region
- one or two best-value examples
- a short verdict per region covering fit and tradeoffs

### Phase 6: Maintain

Use these to verify and prune the working set:

```bash
"$HOME/.tools/let/let" health
"$HOME/.tools/let/let" sources status
"$HOME/.tools/let/let" inspect <rightmove-id> --refresh all
"$HOME/.tools/let/let" start --id <rightmove-id>
```

Verification notes:

- Use `--refresh all` when a listing's external evidence may have changed.
- Rebuild sources when `health` or `sources status` reports missing DBs that matter to the current decision.
- Preserve evidence gaps in the final report instead of patching unverified values into the DB.
- Use `start` for local browsing only after evidence has been inspected; it launches the TUI against the existing intelligence DB and cache.

## Viewing Coordination With `gog`

Use this section only when the user wants help scheduling or tracking viewings and explicitly allows email or calendar access.

Rules:

- Confirm `gog` is installed and authenticated before using it.
- Never hardcode account emails, calendar IDs, or private naming conventions into this skill.
- If the user wants a calendar event and has not named a calendar, ask which calendar to use.
- If the user wants you to match an existing viewing-event convention, inspect nearby events on the chosen calendar first.
- Verify the listing identity before touching Gmail or Calendar. Prefer the Rightmove portal ID; if the request is ambiguous, resolve it from `let evidence <id>` or the recent email context.

Suggested workflow:

1. Identify the property and confirm the portal ID, address, listing URL, and agent.
2. If the user explicitly allows inbox access, search recent mail with `gog gmail search` using the portal ID, address fragments, agent name, and terms like `viewing`, `confirmed`, or `appointment`.
3. Read the most relevant thread with `gog gmail thread get <threadId> -j` and extract the confirmed date, time, contact name, address, and any special instructions.
4. If the user explicitly allows calendar access, list calendars with `gog calendar calendars -j`, ask which calendar to use unless already specified, and inspect nearby events with `gog calendar events <calendarId> ... -j` to infer the local title, description, duration, and reminder convention.
5. Create the event only after the booking details are confirmed. Re-read the target day afterwards to confirm the event exists and to spot obvious duplicates or overlaps.

Default title pattern:

- `Viewing: <short address or road> w/ <agent short name>`
- Use the street or short address fragment that matches the nearby viewing events on the chosen calendar.
- Keep the title terse and comparable across listings so the day view stays scannable.

Default description pattern:

- Start with one confirmation line naming the source and date.
- Include the listing link, portal ID, rent, beds and baths, availability, agent contact, area metrics, stored assessment snapshot, booking confirmation line, exact EPC reference if known, and a one-line summary.
- Omit unknown fields instead of inventing data.
- Prefer this field order and punctuation so new events match the existing viewing entries closely.
- Default viewing duration to `15` minutes when the confirmation email gives a start time but no duration and there is no stronger local convention on the calendar.

Suggested description skeleton:

```text
Viewing confirmed by <agent or sender> email on <DD Mon YYYY>.

Rightmove: <listing-url>
Listing: <portal-id> | <property type> | <beds> bed | <baths> bath | <rent> pcm | Available <date-or-now>
Agent: <agent name> | <agent phone>
Area: <postcode> | <nearest station> <distance> | EPC <rating> | <floor area> sqm | Gigabit <pct>% | Crime <rate>/1k | IMD <decile> | Flood <level> | Social housing <pct>
Stored assessment: <recommendation> | Confidence <confidence> | Priority <priority>
Booking: Confirmed by <contact name> for <weekday> <D Mon YYYY> at <time>
Exact EPC: <full address> | Cert <certificate-number> | Valid until <date>
Summary: <one-line verdict>
```

Example create:

```bash
gog calendar create <calendar-id> \
  --summary 'Viewing: <short address> w/ <agent>' \
  --from '<RFC3339>' \
  --to '<RFC3339>' \
  --location '<full address>' \
  --visibility private \
  --source-title 'Rightmove listing' \
  --source-url '<listing-url>' \
  --description $'Viewing confirmed by <agent or sender> email on <DD Mon YYYY>.\n\nRightmove: <listing-url>\nListing: <portal-id> | <property type> | <beds> bed | <baths> bath | <rent> pcm | Available <date-or-now>\nAgent: <agent name> | <agent phone>\nArea: <postcode> | <nearest station> <distance> | EPC <rating> | <floor area> sqm | Gigabit <pct>% | Crime <rate>/1k | IMD <decile> | Flood <level> | Social housing <pct>\nStored assessment: <recommendation> | Confidence <confidence> | Priority <priority>\nBooking: Confirmed by <contact name> for <weekday> <D Mon YYYY> at <time>\nExact EPC: <full address> | Cert <certificate-number> | Valid until <date>\nSummary: <one-line verdict>'
```

## Postcode and Neighborhood Research

When an evidence bundle includes a usable UK postcode, fetch these postcode sources with WebFetch as a standard part of shortlist assessment:

- `https://area360.uk/postcode/{POSTCODE}`
- `https://www.streetcheck.co.uk/crime/{POSTCODE}`
- `https://crystalroof.co.uk/report/postcode/{POSTCODE}/overview`

Normalization:

- Remove spaces for the URL.
- Use the compact token as `{POSTCODE}` in all three URLs.
- Example: `SY2 6BB` becomes `SY26BB`.

What to extract from the fetched pages:

- From `area360.uk`: broad postcode context such as crime, deprivation, flood risk, noise, station distance, schools, parks, amenities, and local price context.
- From `streetcheck.co.uk`: crime detail, overall crime level, notable category spikes, and nearby recent incidents that materially affect fit.
- From `crystalroof.co.uk`: deprivation, income or affluence signals, noise, flood, transport, amenities, and school context.
- Use housing mix, tenure, and demographics only when they materially affect fit.

Rules:

- Treat these postcode pages as standard context sources for shortlist candidates, not optional extras.
- Use the fetched pages for textual signals. Do not assume interactive maps or charts are visible in WebFetch output.
- Combine overlapping signals proportionately. Do not double-count the same crime or deprivation pattern just because it appears on multiple sites.
- Separate listing quality from location quality in the assessment.
- Call out major positives, major negatives, and any dealbreakers.
- If the postcode is missing or one or more pages are unavailable, say so and lower confidence accordingly.

## Deterministic Scores

Deterministic score code may exist as an internal experiment, but the default decision layer is the AI-authored assessment over evidence. Do not rely on score commands as part of the core workflow.

### Score Interpretation

| Range | Meaning | Default Action |
| --- | --- | --- |
| 85-100 | Exceptional | Must assess |
| 70-84 | Good | Assess |
| 55-69 | Average | Assess if time permits |
| 40-54 | Below average | Usually skip |
| < 40 | Poor | Skip |

### Composite Model

- Affordability, default 30%: rent plus estimated heating and price percentile.
- Location, default 40%: station proximity, broadband, region priority, IMD, crime.
- Liveability, default 30%: garden type, heating type, property type.
- Penalties such as EPC, garden, and pets apply multiplicatively after composite aggregation.
- Treat this model as background context only unless deterministic scoring is explicitly reintroduced into the active workflow.

## Subagents

### Search and Inspect by Location

Use sequential subagents for multi-region exploration.

1. Main agent orients, decides locations, and sets batch sizes.
2. Spawn one location subagent.
3. Wait for completion.
4. Spawn the next location subagent.

When delegating a location, give the subagent:

- the summary from `$HOME/.tools/let/data/let.context.md`
- the current user request
- the location name and identifier if known
- the rule set: no config edits, use overrides if needed, keep batches small, write assessments back normally

Template:

```text
You are a subagent exploring one location for the `let` property search.

Read first: $HOME/.tools/let/data/let.context.md.

Constraints:
* Do not edit config files.
* Use default command output for tool calls. Expect JSON envelopes for all structured commands.
* Keep the batch small: discover, then inspect 2-5 promising IDs.
* Verify checkable claims before treating them as true.
* Write assessments back with `assess save`.
* Return a compact summary.

Location:
* Name: {LOCATION}
* Identifier: {LOCATION_ID}

Steps:
1) `"$HOME/.tools/let/let" health`
2) Discover listings for this location
3) Inspect a small candidate batch
4) Read evidence and verify broadband/EPC/address claims where relevant
5) Deep dive 1-2 using photos, maps, postcode research sources, and quick neighborhood research
6) Save 1-2 assessments
7) Return:
8) Return:
   * Top 3 candidates with short rationale
   * Red flags
   * Overrides used
   * Short verdict on fit for the family's goal
```

### Assessment-Only Parallelism

Parallel assessment subagents are allowed only for disjoint listing IDs. Each subagent should own 2-3 listings max and submit only for its assigned IDs.

## Error Handling

Error codes appear in `error.code` when `ok: false`.

| Code | Meaning | Recovery Action |
| --- | --- | --- |
| `NO_CONFIG` | Config missing | Create from `.claude/skills/let/templates/let.config.toml`, then re-run health |
| `NO_SOURCES` | Source DBs missing | Proceed degraded or run `"$HOME/.tools/let/let" sources build all --jobs 3` |
| `SCHEMA_MISMATCH` | DB schema incompatible | Delete `let.db`, then run `"$HOME/.tools/let/let" inspect <id>` to recreate it |
| `CONFLICT` | Database lock contention | Close competing DB users, then retry the command |
| `NOT_FOUND` | Listing removed | Skip and continue |
| `VALIDATION_ERROR` | Invalid assessment or input data | Fix according to schema and `error.hint` |
| `DB_ERROR` / `INTERNAL_ERROR` | Database read/write or path failure | Run `"$HOME/.tools/let/let" health"`; check permissions, locks, and disk state before restoring or recreating anything |
| `NETWORK_ERROR` | Network request failed | Check connectivity/TLS, then retry once |
| `PARSE_ERROR` | Upstream payload parse failed | Retry once; if persistent, treat as upstream drift and continue degraded |
| `NO_CREDENTIALS` | API credentials missing | Set keys in `$HOME/.tools/let/data/.env` and re-run health |
| `INVALID_DB` | Notion database inaccessible | Check Notion credentials and DB ID |

Exit codes:

| Code | Meaning |
| --- | --- |
| `0` | Success, including partial success |
| `1` | Runtime error |
| `2` | Prerequisites blocked |

Recovery rules:

1. Read `error.code` and `error.hint`.
2. Apply the table action.
3. If the error persists, re-run `"$HOME/.tools/let/let" health"` to check for systemic issues.
4. For rate limits, back off and never retry more than twice overall.
5. For missing enrichment or media, continue with lower confidence instead of blocking the run.

## Intent Mappings

- Top 5 new homes: baseline mode, discover, inspect a small candidate batch, verify claims, assess top 2-3, report top 5.
- Compare Manchester, Liverpool, Sheffield: override mode per city, inspect small samples, summarize per region, then compare.
- Flats around York within about 30 minutes of the centre: override property type to flats, use nearby towns if supported, and label any travel-time approximation clearly.

## Expected Output

When the user asks you to search, return:

- a region-by-region comparison with fit, value, and tradeoffs
- a shortlist of the top 3-5 listings with links and clear rationale
- a short location verdict for shortlist items using postcode context when available
- suggested config refinements when you observed drift
- a brief summary of what you actually did
