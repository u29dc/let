# Initialization Guide

Setup wizard for first-run configuration. Work through each section in order; re-run `bin/let health --json` at the end to confirm.

## 1. Configuration

The config file controls search locations, filters, and scoring weights. If `bin/let health --json` reports `NO_CONFIG`, create one from the template.

### Conversational setup

Ask the user these questions to build the config:

1. **"What areas are you considering?"** -- Each area needs a location identifier. Use `bin/let search resolve <name> --json` to resolve city/town names to REGION IDs. Example: `bin/let search resolve Sheffield --json` returns `REGION^904`.

2. **"Houses, flats, or both?"** -- Maps to `propertyTypes` in `[search.filters]`. Options: `detached`, `semi-detached`, `terraced`, `flat`, `apartment`, `house`, `bungalow`, `cottage`, `studio`.

3. **"Budget range?"** -- Sets `minPrice` and `maxPrice` (monthly rent in GBP) in `[search.filters]`.

4. **"How many bedrooms?"** -- Sets `minBedrooms` and `maxBedrooms`.

5. **"Is a garden important?"** -- If yes, add `"garden"` to `mustHave`. This also activates the no-garden scoring penalty.

6. **"Any deal-breakers?"** -- Maps to `dontShow` (e.g., `houseShare`, `retirement`, `student`).

### Config file location

Write the config to `.let/data/let.config.toml` (relative to repo root). The template is at `templates/config.toml`. Key sections:

- `[search]` -- `locations` array of `{ id, name }` pairs
- `[search.filters]` -- bedrooms, price range, property types, mustHave, dontShow
- `[fetch]` -- `delayMs` (default 3000ms between requests), `maxListings`, `maxRetries`
- `[scoring]` -- adaptiveness, composite weights, penalty multipliers
- `[scoring.regionPriority]` -- personal preference scores per region (0-100)

### Region priority

After resolving locations, ask: **"How would you rank these areas by preference?"** Assign scores 0-100 in `[scoring.regionPriority]`. Higher scores make listings in that region rank higher.

## 2. API keys

Create a `.env` file in the data directory (`.let/data/.env`) with the following keys:

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

## 3. Source databases

Source databases provide local enrichment data (broadband speeds, deprivation indices, crime stats, flood risk, census data). They live in the `.let/sources/` directory.

### What "degraded" means

Without source databases, the pipeline still works end-to-end. What changes:

- No broadband speed data (gigabit availability unknown)
- No IMD deprivation ranking
- No crime statistics
- No flood risk assessment
- No census tenure or population data
- No income estimates

Scores will have lower confidence and the location composite will be less accurate. The core pipeline (fetch, parse, score by price/EPC/property type) remains fully functional.

### Options

**(a) Proceed degraded** -- Recommended for quick starts. The pipeline works; enrichment data can be added later.

**(b) Build from source** -- Requires the full repository:

```bash
bun install
bun run build:sources    # downloads ~5-10GB from government sources
```

Building takes 10-30 minutes depending on network speed. Some government data URLs may expire; the build logs which sources succeeded.

## 4. Final verification

Run the health check to confirm everything is configured:

```bash
bin/let health --json
```

Expected outcomes:

- `status: "ready"` -- All prerequisites met, full enrichment available
- `status: "degraded"` -- Pipeline works but some enrichment data is missing (acceptable)
- `status: "blocked"` -- Critical prerequisite missing; check `checks[].fix` for remediation commands

If blocked, address each failing check using the provided fix commands, then re-run health.
