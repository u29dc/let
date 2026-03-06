# Initialization Guide

Setup wizard for first-run configuration. Work through each section in order; re-run `"$LET_BIN" health --json` at the end to confirm.
Set once per session: `LET_BIN="${LET_HOME:-${TOOLS_HOME:-$HOME/.tools}/let}/let"`.
If `"$LET_BIN"` is missing or not executable, return a blocked prerequisite and stop.

## 1. Configuration

The config file controls search locations, filters, and scoring weights. If `"$LET_BIN" health --json` reports `NO_CONFIG`, create one from the template.

### Conversational setup

The setup has two phases: understanding the person, then translating into config. Start open-ended, then narrow down to specifics. The open-ended answers feed the context file (`$LET_HOME/data/let.context.md`); the specific answers feed the config TOML.

### Phase 1: Understanding

These questions don't map directly to config fields -- they capture what the numbers can't. Ask naturally, not as a checklist.

- **"What does the perfect home look like for you?"** -- Lets them describe the dream without constraints. Listen for vibe cues (character vs modern, quiet vs buzzy, rural vs urban)
- **"What do you love about where you live now?"** -- Reveals what to preserve. Maybe it's the park next door, the walk to the high street, the light in the morning
- **"What's the one thing you'd change?"** -- Surfaces the main pain point driving the move (no garden, too expensive, wrong area, outgrowing the space)
- **"Describe your ideal neighborhood in a sentence."** -- Forces prioritization. "Walkable, leafy, near good schools" is very different from "cheap, spacious, doesn't matter where"
- **"Who's moving?"** -- Family size, ages, pets, remote work, commute needs. A couple with a toddler and a planned dog has different needs than a solo remote worker

Use the answers to write/update the context file (see "User context file" below). These answers also inform how you set region priorities, scoring weights, and which tradeoffs to flag during searches.

### Phase 2: Config specifics

Ask these to populate the TOML fields:

- **"What areas are you considering?"** -- Each area needs a location identifier. Use `"$LET_BIN" search resolve <name> --json` to resolve city/town names to REGION IDs. Example: `"$LET_BIN" search resolve Sheffield --json` returns `REGION^904`
- **"Houses, flats, or both?"** -- Maps to `propertyTypes` in `[search.filters]`. Options: `detached`, `semi-detached`, `terraced`, `flat`, `apartment`, `house`, `bungalow`, `cottage`, `studio`
- **"Budget range?"** -- Sets `minPrice` and `maxPrice` (monthly rent in GBP) in `[search.filters]`
- **"How many bedrooms?"** -- Sets `minBedrooms` and `maxBedrooms`
- **"Is a garden important?"** -- If yes, add `"garden"` to `mustHave`. This also activates the no-garden scoring penalty
- **"Any deal-breakers?"** -- Maps to `dontShow` (e.g., `houseShare`, `retirement`, `student`)

Many Phase 2 answers will already be implicit from Phase 1. Don't re-ask what's already clear -- just confirm and fill in gaps.

### Config file location

Write the config to `$LET_HOME/data/let.config.toml`. The template is at `templates/let.config.toml`. Key sections:

- `[search]` -- `locations` array of `{ id, name }` pairs
- `[search.filters]` -- bedrooms, price range, property types, mustHave, dontShow
- `[fetch]` -- `delayMs` (default 3000ms between requests), `maxListings`, `maxRetries`
- `[scoring]` -- adaptiveness, composite weights, penalty multipliers
- `[scoring.regionPriority]` -- personal preference scores per region (0-100)

### Region priority

After resolving locations, ask: **"How would you rank these areas by preference?"** Assign scores 0-100 in `[scoring.regionPriority]`. Higher scores make listings in that region rank higher.

### User context file

After the conversational setup, write or update `$LET_HOME/data/let.context.md` with a prose summary of the user's situation. This file gives future agent runs the human context behind the config numbers. It should capture:

- Family composition, life stage, pets (current or planned)
- Current living situation -- what they pay, what they love about it, what's missing
- What they're looking for in a new place (vibe, character, nature access, walkability)
- Non-negotiables and acceptable tradeoffs
- How they think about scoring (what 100/100 means to them)

**If the file doesn't exist**: create it from the conversation. Ask follow-up questions if needed -- "What do you love about where you live now?", "What's the one thing you'd change?", "Describe your ideal neighborhood in a sentence."

**If the file already exists**: read it and check whether the conversation revealed anything new or changed (budget shift, new must-have, different regions, life change). If so, update the relevant parts. Don't rewrite what's already accurate.

The context file is personal data in `$LET_HOME/data/`. Write in natural prose, not bullet config. The goal is that any agent reading it cold understands the family's priorities without needing to re-ask.

## 2. API keys

Create a `.env` file in the data directory (`$LET_HOME/data/.env`) with the following keys:

### EPC_API_KEY (recommended)

Required for energy rating, floor area, and UPRN enrichment. Without it, EPC data is skipped and scoring confidence is lower.

1. Register at https://epc.opendatacommunities.org
2. Request an API key (free, instant)
3. Add to `.env`: `EPC_API_KEY=your_key_here`

### MAPBOX_ACCESS_TOKEN (optional)

Enables satellite and street map views cached as WebP images. Useful for neighborhood assessment but not required for the core pipeline.

1. Create account at https://www.mapbox.com
2. Generate a public access token
3. Add to `.env`: `MAPBOX_ACCESS_TOKEN=your_token_here`

### Notion export keys (optional)

Required only for `"$LET_BIN" export notion ...`.

1. Create a Notion integration and copy the API key
2. Create/select a Notion database and copy its database ID
3. Add to `.env`:
   - `NOTION_API_KEY=your_notion_integration_key`
   - `NOTION_DATABASE_ID=your_notion_database_id`

## 3. Source databases

Source databases provide local enrichment data (broadband speeds, deprivation indices, crime stats, flood risk, census data). They live in the `$LET_HOME/sources/` directory.

### Degraded mode

Without source databases, the pipeline still works end-to-end. What changes:

- No broadband speed data (gigabit availability unknown)
- No IMD deprivation ranking
- No crime statistics
- No flood risk assessment
- No census tenure or population data
- No income estimates

Scores will have lower confidence and the location composite will be less accurate. The core pipeline (fetch, parse, score by price/EPC/property type) remains fully functional.

### Options

- **Proceed degraded** -- recommended for quick starts. The pipeline works; enrichment data can be added later
- **Build source databases locally** -- from the installed CLI:

```bash
"$LET_BIN" build sources all --jobs 3    # downloads ~5-10GB from government sources
```

Building takes 10-30 minutes depending on network speed. Some government data URLs may expire; the build logs which sources succeeded.

## 4. Final verification

Run the health check to confirm everything is configured:

```bash
"$LET_BIN" health --json
```

Expected outcomes:

- `status: "ready"` -- all prerequisites met, full enrichment available
- `status: "degraded"` -- pipeline works but some enrichment data is missing (acceptable)
- `status: "blocked"` -- critical prerequisite missing; check `checks[].fix` for remediation commands

If blocked, address each failing check using the provided fix commands, then re-run health.
