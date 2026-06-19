# Agent-Native Property Intelligence Rebuild Plan

## 1. Purpose

Rebuild `let` as a clean agent-native UK rental property intelligence toolkit.

The new system must give an AI agent one concise way to inspect a listing holistically, while preserving modular internal failure boundaries. The tool should gather, preserve, reconcile, and verify evidence. The agent should make the final assessment.

This is a full refactor plan for a future implementation agent. It intentionally does not preserve legacy command compatibility. Existing useful source integrations and parsing logic should be reused where they still fit, but the current pipeline-shaped command surface and listing-centric database should be replaced.

## 1A. Current Decision

Proceed with the agent-native rebuild, but do it as a staged hard cut, not as a compatibility migration.

The correct shape is:

- A small public command surface that an agent can discover with `let tools`.
- One holistic command, `let inspect`, for normal listing intelligence.
- A few targeted companion commands, `evidence`, `verify`, `correct`, `assess`, `sources`, and `start`.
- Modular SDK internals for capture, extraction, resolution, facts, verification, media, corrections, and persistence.
- No public legacy wrappers for `fetch`, `view`, `score`, `ops`, or `export`.

The reason is practical: agents need enough structure to request a complete listing picture without assembling 50 tiny provider calls, but they also need section-level failure, refresh, and correction boundaries. `inspect` should feel simple from the outside and decomposed inside.

The next implementation agent should continue from the current worktree. Do not restart the refactor. The first working slice already exists and has been validated:

- The public command surface has been narrowed.
- The intelligence DB is initialized by `inspect`.
- `inspect`, `evidence`, `verify`, `assess save/get`, `sources`, and `start` exist.
- Media download and normalization are wired into `inspect --section media`.
- `verify --refresh` no longer downloads media unless the requested claim is media.
- Evidence no longer reports a local media path for a missing cache file.

Treat this plan as both the target architecture and the remaining implementation backlog.

## 2. Consolidated Product Direction

### Goal

Create a comprehensive toolkit that gives an AI agent complete property intelligence for a listing:

- Raw and normalized Rightmove evidence.
- Photos, floorplans, EPC images, maps, and local media paths.
- Address, postcode, coordinate, UPRN, and EPC candidate resolution.
- Source-backed area facts: broadband, postcode geography, deprivation, census, population, income, flood, crime, NaPTAN, UPRN.
- Claim extraction from listing text.
- Claim verification against independent facts.
- Explicit conflicts, ambiguity, confidence, missing inputs, and retry paths.
- A stable evidence bundle that an agent can assess without scraping CLI text.

### Balance

Avoid both extremes:

- Not two commands that hide every failure in an opaque pipeline.
- Not 50 commands that force the agent to hand-assemble a single listing.

The public surface should be small. The internal architecture should be modular.

### Core Principle

Expose sections, not tiny stage commands.

The main user-facing command should be `let inspect <id-or-url>`. It should run capture, extraction, resolution, fact collection, claim verification, and media handling as internal stages. Each stage returns a section status, provenance, confidence, warnings, and targeted next actions.

## 3. Current Codebase Audit

### Current Strengths To Preserve

- JSON-first envelope with optional Toon in `crates/let-cli/src/envelope.rs`.
- Registry-backed `let tools` surface in `crates/let-cli/src/registry.rs`.
- `let health` readiness checks in `crates/let-cli/src/commands/health.rs`.
- Path precedence and runtime directory separation in `crates/let-sdk/src/paths.rs`.
- Source database builders in `crates/let-sdk/src/sources/`.
- Source metadata writing in `crates/let-sdk/src/sources/common.rs`.
- Rightmove PAGE_MODEL extraction and status classification in `crates/let-sdk/src/pipeline/fetch/rightmove.rs`.
- Rightmove search API plus HTML fallback in `crates/let-cli/src/commands/search.rs`.
- EPC modern and legacy API support in `crates/let-sdk/src/pipeline/epc.rs`.
- Media normalization, hashing, cache naming, map image support, and concurrency controls in `crates/let-sdk/src/pipeline/fetch/media.rs` and `cache.rs`.
- UPRN candidate logic in `crates/let-sdk/src/pipeline/uprn.rs`.
- NaPTAN station fallback logic in `crates/let-sdk/src/pipeline/naptan.rs`.
- Deterministic scoring logic in `crates/let-sdk/src/pipeline/score.rs`, if retained as optional evidence.
- Strong integration tests around envelopes, Toon, schema mismatch, health, validation, and current CLI contracts.

### Current Problems To Fix

#### `fetch` is a workflow mega-command

`crates/let-cli/src/commands/fetch.rs` currently does all of this in one path:

- Parses Rightmove IDs.
- Loads config and DB.
- Fetches Rightmove.
- Applies address and postcode overrides.
- Geocodes override coordinates.
- Opens local source DBs.
- Enriches listing fields.
- Performs EPC lookup.
- Resolves UPRN.
- Backfills stations from NaPTAN.
- Computes scores.
- Applies min-score filtering before media.
- Downloads and normalizes media.
- Recomputes scores.
- Deduplicates listings.
- Persists to the listings DB.

This mixes evidence gathering, data mutation, scoring, filtering, media, persistence, and workflow policy. It should be deleted or rewritten into internal services behind `inspect`, not kept as a public compatibility command.

#### The DB is listing-centric instead of evidence-centric

`crates/let-sdk/src/db/schema.sql` stores one canonical `listings` row with many nullable columns. Enrichment mutates fields directly. This loses source-level evidence and makes conflicts difficult to inspect.

The new DB should store raw source snapshots, observations, candidates, facts, claims, verifications, media assets, and assessments separately.

#### Description handling is lossy

`rightmove.rs` currently builds a single lowercase sanitized description by merging `keyFeatures` and `text.description`. The rebuild must preserve:

- Raw HTML or raw text from Rightmove.
- Key features as separate items.
- Plain text with formatting removed.
- Normalized text for claim extraction.
- Source spans or field references for extracted claims.

#### Broadband facts are underused

The broadband source DB contains postcode, outward, area, LAUA, take-up, and national summary tables, but current enrichment only writes a single `gigabit_availability` value. The rebuild must expose richer broadband facts and verify textual claims such as "gigabit broadband" against source data.

#### Mapbox geocoding is too shallow

Current Mapbox usage returns only the first candidate coordinate. The rebuild must store candidate lists with confidence, match reasons, place type, address text, coordinates, and conflicts.

#### EPC matching hides ambiguity

The EPC module already searches address-scoped and postcode-scoped candidates, but the public output collapses to a single `EpcLookup`. The rebuild must expose candidates, selected certificate, match score, ambiguity, and mismatch reasons.

#### Registry schemas are empty

The live `let tools` catalog currently reports `inputSchema: null` and `outputSchema: null` for every tool. The rebuild must make schema metadata mandatory for all structured commands.

#### Health checks are tied to old commands

`health` currently points DB fixes at `let fetch <id>`. It must be updated for the new evidence DB and source health model.

### Runtime QA Findings From 2026-06-18

These findings came from the production binary at `/Users/han/.tools/let/let` after the intelligence DB had been deleted.

Runtime initialization:

- `let inspect 89872542 --depth standard` recreated `/Users/han/.tools/let/data/let.db`.
- The recreated DB contains the new intelligence tables: `entities`, `entity_identifiers`, `source_snapshots`, `facts`, `claims`, `verifications`, `media_assets`, `evidence_bundles`, `assessments`, `address_candidates`, and `address_resolutions`.
- `let health` then reported the intelligence DB as OK with one entity and one bundle.
- `let sources build all --jobs 3 --progress plain` rebuilt all 10 source DBs successfully: broadband, postcodes, deprivation, census, population, income, flood, NaPTAN, UPRN, and crime.
- After source rebuild, `let sources status` reported `present: 10` and `missing: 0`.
- `bun run build` completed and installed both `let` and `let-tui` into `/Users/han/.tools/let`.

Observed functional gaps:

- `let start` was restored in the next implementation slice as a thin `let-tui` launcher; remaining TUI work is evidence-DB projection and local media rendering.
- `inspect --depth standard --section media` now downloads and normalizes Rightmove photos, and captures Mapbox static maps when `MAPBOX_ACCESS_TOKEN` is available. A live smoke for listing `89872542` produced 12 local photos and 2 local maps.
- Remaining media gaps: TUI now has an evidence-bundle projection for listings and media paths, but still needs richer section screens, floorplan/EPC PDF-aware behavior, remote-only media states, and per-asset failure reasons.
- `/Users/han/.tools/let/data/let.config.toml` still parses the old `[fetch]` media and scoring shape. `search` uses the search config; `inspect` currently loads only `FetchConfig` for retry behavior. Media config, map download config, min-score config, and most scoring config are not wired into the new evidence workflow.
- After source rebuild, broadband and local area facts populate again, but address resolution for `CT21 5QR` still reports `postcode was not found in the local postcode database`. This needs a focused postcode normalization/lookup bug check.
- `verify --claim broadband` works mechanically, but returns no verifications for a listing whose description has no broadband claim. The claim extractor path should be tested with a fixture or live listing that mentions `gigabit`.

Review fixes applied after the first QA pass:

- `verify --claim broadband --refresh all` now refreshes only Rightmove, description, claims, broadband, and verifications. It no longer runs the standard inspect default sections and no longer downloads media as a hidden side effect.
- Explicit `verify --claim media --refresh all` still includes media.
- Media evidence now sets `localPath` only when the computed cache file exists. Stale or missing local files remain `status: "remote"` and do not inflate cached-media counts.
- `bun run util:check` passed after the fixes and reinstalled `/Users/han/.tools/let/let` and `/Users/han/.tools/let/let-tui`.
- Production smoke confirmed broadband verification leaves a fresh temp cache empty.
- Production smoke confirmed explicit media inspection for listing `89872542` returns 12 cached local photos and 12 cache files.

Implementation slice applied after the plan update:

- `let correct address`, `let correct epc`, `let correct media`, and `let correct clear` are now implemented as registry-backed commands.
- Corrections are stored in an append-only `corrections` table and are surfaced in `evidence` bundles.
- Active address corrections add a `manualCorrection` address candidate, can override postcode for broadband lookup, and can provide coordinates for map generation.
- Active EPC corrections can pin EPC evidence using LMK key, certificate URL, UPRN, rating, and floor area.
- Active media corrections can provide map coordinates used before map media generation.
- `let-tui` now attempts to load listings from intelligence evidence bundles before falling back to the legacy listing loader, and honors `LET_START_ID` / `LET_START_SECTIONS` focus inputs from `let start`.

Validation applied after this slice:

- `bun run util:check` passed: formatting, clippy with `-D warnings`, `cargo check`, all Rust tests, and release install.
- Production `let --help` shows the narrowed command surface including `correct` and `start`.
- Production `let search resolve sevenoaks` resolved `REGION^1191`.
- Production `let search discover` returned zero results under the live narrow budget config, then returned three Sevenoaks ids with a copied broader temp config, confirming filter behavior and HTML fallback discovery.
- Production `let inspect 173582396 --depth standard --refresh all` in an isolated temp runtime captured Rightmove evidence, broadband facts, 7 local photos, 2 local maps, and 11 cached media files.
- Production `correct address`, `correct epc`, `correct media`, and `correct clear` all worked against that temp listing. Re-inspect consumed active corrections for address candidates, broadband postcode, EPC pinning, and Mapbox map generation.
- Production `verify 173582396 --claim broadband --refresh all` refreshed without writing media cache files.
- Production `assess save/get` round-tripped agent-authored JSON.
- Production `sources status` reported all 10 source DBs present. `health` is only degraded by missing Notion credentials, not property intelligence sources.

The next agent should treat this as the immediate stabilization backlog before adding new features.

Current implementation status:

| Area | Status | Next action |
| --- | --- | --- |
| Command surface | Landed | Keep old commands unsupported and registry schemas non-null |
| Evidence DB | Landed as schema version 1 with additive corrections | Add richer observations, source snapshot persistence, and stricter bundle versioning |
| Rightmove inspect | Landed | Preserve more raw fields, description variants, and source paths |
| Media inspect | Landed for photos and maps | Persist per-asset failure reasons, handle floorplan/EPC PDF assets, project media into TUI |
| Broadband facts | Partially landed | Expose richer Ofcom records and add gigabit/full-fibre fixtures |
| Verify | Landed, targeted refresh fixed | Add address/EPC/media/description verifiers and correction-aware reasoning |
| Address resolution | Basic plus manual correction candidate | Add candidate graph, postcode normalization audit, Mapbox candidates, and UPRN ambiguity |
| EPC | Basic plus manual pinning | Store candidate list, ambiguity reasons, and conflict checks |
| Config | Old shape still active | Migrate `[fetch]` into `[inspect]` and `[media]`; migrate scoring into optional `[score]` |
| `start`/TUI | Evidence-bundle loader started | Finish section UI, warnings/facts/claims display, and remote-only media states |

## 4. Target Public Command Surface

Keep the public command surface concise.

### Required Commands

```text
let tools [name]
let health
let config show

let search resolve <location>
let search discover [filters]

let inspect <id-or-url> [--depth quick|standard|deep] [--refresh none|stale|all] [--section <section-list>]
let evidence <id> [--section <section-list>]
let verify <id> [--claim all|address|broadband|epc|media|description] [--refresh none|stale|all]
let correct address <id> --address <text> [--postcode <postcode>] [--lat <lat> --lng <lng>] [--note <text>]
let correct epc <id> [--certificate-url <url>] [--lmk-key <key>] [--uprn <uprn>] [--rating <band>] [--floor-area-sqm <sqm>] [--note <text>]
let correct media <id> [--map-lat <lat> --map-lng <lng>] [--note <text>]
let assess save <id> <assessment-json>
let assess get <id>
let sources list|status|build <name|all>
let start [--id <id>] [--section <section-list>]
```

### Commands To Remove From The Public Surface

Remove or replace these current public commands:

- `let fetch`
- `let view list`
- `let view detail`
- `let score compute`
- `let score explain`
- `let assess candidates`
- `let assess context`
- `let assess submit`
- `let ops patch`
- `let ops prune`
- `let ops verify`
- `let export json`
- `let export notion`
- `let build sources`

Replacement intent:

- `fetch` becomes internal `inspect` orchestration.
- `view` becomes `evidence`.
- `score` becomes an optional evidence section, not a default gate.
- `assess submit/get` becomes `assess save/get`.
- `ops verify` becomes `verify`.
- `build sources` becomes `sources build`.
- Export can be revisited after the evidence model stabilizes.
- TUI remains part of the product. `let start` should launch `let-tui` against the evidence DB once the DB and media manifest are rebuilt.

Do not reintroduce generic `ops patch` as a compatibility wrapper. The replacement is a small `correct` command family with explicit provenance and dependency refresh semantics.

### Command Design Rationale

`inspect` is the main command for agents. It should run the internal five-layer workflow and return one complete evidence bundle.

`evidence` is read-only. It returns cached evidence, either complete or section-scoped.

`verify` is a focused retry/recheck command. It exists when the agent sees a failed, stale, contradicted, or ambiguous section and needs targeted recomputation without rerunning all inspection work.

`assess` stores agent judgment only. It must not gather facts or compute hidden recommendations.

`sources` manages local source DBs and their freshness.

`correct` stores user- or agent-supplied corrections as first-class evidence, not as destructive rewrites of Rightmove data. Corrections must be visible in `evidence`, must record who/what supplied them, and must trigger targeted re-resolution of dependent sections.

`start` is the local visual browser. It should be thin process launch and path resolution, not a second data model.

## 5. Internal Five-Layer Model

The public surface can stay small because the SDK should expose these internal layers:

```text
capture -> extract -> resolve -> verify -> assess
```

### 5.1 Capture

Capture stores raw source responses and local media.

Responsibilities:

- Fetch Rightmove listing HTML.
- Extract and store `PAGE_MODEL`.
- Fetch Rightmove search results.
- Fetch EPC search and certificate responses.
- Fetch Mapbox geocode responses.
- Query local source DBs and record the rows or result records used.
- Download and normalize media assets.
- Record HTTP status, content type, fetched timestamp, URL, provider, request parameters, and body hash.

Capture must not decide truth. Capture records source evidence.

### 5.2 Extract

Extract turns raw source payloads into typed observations.

Responsibilities:

- Parse Rightmove property data.
- Preserve raw and normalized descriptions.
- Extract key features, price, rent period, beds, baths, property type, availability, deposit, agent, media URLs, coordinates, pin type, listing status, and listed date.
- Extract EPC candidate fields.
- Extract Mapbox candidate fields.
- Extract source DB fact records.
- Extract text claims from descriptions and features.

Extraction should produce observations with source references, not overwrite canonical fields.

### 5.3 Resolve

Resolve synthesizes a canonical view from observations and candidates.

Responsibilities:

- Resolve listing identity.
- Resolve address, postcode, coordinates, UPRN, and building/unit ambiguity.
- Select EPC certificate or mark ambiguity.
- Resolve nearest stations.
- Produce canonical facts only when confidence is adequate.
- Preserve alternatives and conflicts.

Resolution must explain its decision. It should never silently collapse multiple flats, wrong postcodes, wrong EPC links, or conflicting coordinates into false certainty.

### 5.4 Verify

Verify compares claims against independent facts.

Responsibilities:

- Compare "gigabit", "full fibre", "fast broadband", and related description claims against Ofcom broadband data.
- Compare EPC claims or links against EPC certificate candidates.
- Compare address claims against Mapbox, EPC, UPRN, postcode DB, and Rightmove coordinates.
- Compare station claims against Rightmove and NaPTAN.
- Compare flood, deprivation, crime, and census claims if such text appears.
- Return `supported`, `contradicted`, `unknown`, or `insufficientEvidence`.

Verification must include source references and confidence.

### 5.5 Assess

Assess stores the agent's final judgment.

Responsibilities:

- Save structured agent assessment.
- Retrieve prior assessment.
- Keep assessment separate from evidence.
- Optionally store agent-visible rationale, photo analysis, concerns, recommendation, and score adjustment.

The tool may provide deterministic scores as evidence, but the agent owns final scoring and recommendation.

## 6. Evidence Bundle Contract

`let inspect` should return a stable evidence bundle. Shape:

```json
{
  "id": "rightmove:170448131",
  "status": "partial",
  "generatedAt": "2026-06-18T12:00:00Z",
  "depth": "standard",
  "sections": {
    "rightmove": { "status": "ok" },
    "description": { "status": "ok" },
    "address": { "status": "ok", "confidence": "probable" },
    "epc": { "status": "degraded", "reason": "ambiguous_candidates" },
    "broadband": { "status": "ok" },
    "facts": { "status": "partial" },
    "claims": { "status": "ok" },
    "media": { "status": "partial" },
    "score": { "status": "skipped" }
  },
  "summary": {
    "address": "Flat 2, 10 Example Street, York YO1 1AA",
    "pricePcm": 1600,
    "bedrooms": 3,
    "propertyType": "Flat",
    "listingStatus": "active",
    "headlineConcerns": [
      "EPC match ambiguous",
      "Rightmove mentions gigabit but postcode availability is low"
    ]
  },
  "rightmove": {},
  "description": {},
  "address": {},
  "epc": {},
  "facts": {},
  "claims": {},
  "verifications": [],
  "media": {},
  "nextActions": []
}
```

### Section Status Values

Use the same values everywhere:

- `ok`: section completed with usable result.
- `partial`: section has usable output but one or more substeps failed.
- `degraded`: prerequisite missing or lower-confidence fallback used.
- `blocked`: required prerequisite missing and no usable result exists.
- `skipped`: intentionally not run due to depth, section filter, or config.
- `stale`: cached evidence exists but refresh policy says it is old.

### Confidence Values

Use:

- `exact`
- `probable`
- `heuristic`
- `ambiguous`
- `unknown`

### Verification Values

Use:

- `supported`
- `contradicted`
- `unknown`
- `insufficientEvidence`

### `nextActions`

Every failed, degraded, stale, or ambiguous section should add an agent-actionable next action:

```json
{
  "section": "epc",
  "action": "verify",
  "command": "let verify 170448131 --claim epc --refresh all",
  "reason": "multiple EPC candidates matched the same postcode"
}
```

## 7. Public Command Details

### 7.1 `let inspect`

Signature:

```text
let inspect <id-or-url> [--depth quick|standard|deep] [--refresh none|stale|all] [--section <section-list>]
```

`id-or-url` accepts:

- Rightmove portal ID.
- Rightmove property URL.
- Future provider IDs if added.

Depth:

- `quick`: Rightmove capture/extract, cached evidence, no expensive media downloads, no deep EPC candidate search unless cached.
- `standard`: default. Rightmove, address resolution, core source facts, claim verification, media manifest, limited media downloads if config permits.
- `deep`: full refresh of provider candidates, media normalization, Mapbox candidate list, EPC candidate search, UPRN ambiguity analysis, all source facts.

Refresh:

- `none`: read cached evidence only; fail or degrade if missing.
- `stale`: refresh missing or stale sections.
- `all`: refresh all selected sections.

Sections:

- `rightmove`
- `description`
- `address`
- `epc`
- `broadband`
- `facts`
- `claims`
- `media`
- `score`
- `all`

Default section set:

- `quick`: `rightmove,description,address,facts,claims`
- `standard`: `rightmove,description,address,epc,broadband,facts,claims,media`
- `deep`: `all`

Mutation semantics:

- `inspect` writes source snapshots, observations, facts, media manifests, verification records, and bundle metadata.
- It is not read-only.
- It should be idempotent with respect to source content hashes, but not strictly idempotent because refresh writes timestamps and snapshots.

Exit behavior:

- Exit `0` when the evidence bundle is produced, including partial bundles.
- Exit `1` for invalid input or unrecoverable runtime failures.
- Exit `2` for blocked prerequisites such as missing config DB schema, unwritable runtime dirs, or required source DBs when selected sections cannot degrade.

### 7.2 `let evidence`

Signature:

```text
let evidence <id> [--section <section-list>]
```

Read-only command. It must not mutate files or DB rows.

Use cases:

- Agent wants to re-open a previous bundle.
- Agent wants a focused media, broadband, EPC, address, or claims section.
- Agent wants source-specific details without rerunning `inspect`.

If requested evidence does not exist, return `ok:false` with `error.code = "not_found"` and a hint to run `let inspect`.

### 7.3 `let verify`

Signature:

```text
let verify <id> [--claim all|address|broadband|epc|media|description] [--refresh none|stale|all]
```

Purpose:

- Retry failed sections.
- Recheck a suspicious claim.
- Refresh stale evidence.
- Isolate costly verification without full `inspect`.

It may mutate verification records and source snapshots depending on refresh mode.

### 7.4 `let assess`

Signatures:

```text
let assess save <id> <assessment-json>
let assess get <id>
```

Assessment is agent-authored judgment. It must reference evidence bundle IDs or source snapshot versions.

Minimum assessment fields:

- `recommendation`
- `familySuitability`
- `maintenance`
- `lightAndSpace`
- `photoAnalysis`
- `neighborhoodAnalysis`
- `tradeoffs`
- `reasoning`
- `concerns`
- `score`
- `scoreAdjustment`
- `evidenceBundleId`

`assess save` validates JSON and persists. It should not recompute facts.

### 7.5 `let sources`

Signatures:

```text
let sources list
let sources status [name|all]
let sources build <name|all> [--jobs N] [--progress auto|plain|off]
```

This replaces `let build sources`.

`sources status` should report:

- DB path.
- Present or missing.
- Source metadata from `source_runs` and `source_inputs`.
- Row counts.
- Declared source version.
- Build timestamp.
- Health status.

### 7.6 `let search`

Keep and clean up current search behavior:

```text
let search resolve <location>
let search discover [--region ...] [--location ...] [--property-types ...] [--must-have ...] [--dont-show ...] [--limit N]
```

Retain API-first plus HTML fallback. Add schemas to registry. Ensure output includes enough metadata for agents:

- IDs.
- Location stats.
- Source mode.
- Truncation.
- Errors per location.
- Search URL or request parameters.

### 7.7 `let correct`

Purpose:

Record manual or agent-supplied corrections when the listing source is wrong, vague, or incomplete.

Required subcommands:

```text
let correct address <id> --address <text> [--postcode <postcode>] [--lat <lat> --lng <lng>] [--note <text>]
let correct epc <id> [--certificate-url <url>] [--lmk-key <key>] [--uprn <uprn>] [--rating <band>] [--floor-area-sqm <sqm>] [--note <text>]
let correct media <id> [--map-lat <lat> --map-lng <lng>] [--note <text>]
let correct clear <id> --kind address|epc|media --correction-id <id>
```

Behavior:

- Corrections are append-only evidence records with provenance, timestamp, author kind, note, and affected fields.
- Corrections never overwrite the raw Rightmove snapshot.
- `correct address` can provide exact address text, exact postcode, exact coordinates, or any subset. It should re-run address resolution, postcode facts, broadband, UPRN, EPC candidate search, map media, and verification sections that depend on the changed fields.
- `correct epc` can pin EPC evidence by certificate URL, LMK key, UPRN, rating, or floor area. It should fetch/store the selected certificate when credentials allow, mark competing candidates as rejected, and re-run EPC-derived facts and verifications.
- `correct media` can override map coordinates or add a known media URL later if needed. It should regenerate affected map assets without touching photo assets.
- `correct clear` disables a correction without deleting it, so old agent decisions remain auditable.
- The response should include `correctionId`, changed sections, skipped sections, warnings, and suggested next commands.

Schema expectations:

- Correction records belong in a dedicated `corrections` table or as typed observations with `source = "manualCorrection"`.
- The evidence bundle should expose active corrections and show which selected address/EPC/media result used each correction.
- `verify` should report when a verification relies on a manual correction.

### 7.8 `let start`

Purpose:

Launch the TUI against the current runtime paths.

Required behavior:

- Resolve the same global path flags as `let`.
- Launch `/Users/han/.tools/let/let-tui` or the sibling installed binary by default.
- Pass through `--data-dir`, `--cache-dir`, `--sources-dir`, and optional starting listing id.
- Exit with a structured envelope if the TUI binary is missing, the DB schema is unsupported, or no readable evidence DB exists.
- Do not scrape, fetch, or mutate listing evidence. TUI data must come from the intelligence DB and media cache.

TUI requirements:

- Show listings/entities from the evidence DB, not the old listing table.
- Show section statuses, warnings, facts, claims, verifications, and saved assessments.
- Render local media paths from `media_assets`.
- Provide image open/preview behavior for photos, floorplans, EPC assets, and maps.
- Clearly distinguish remote-only media from downloaded local media.

## 8. New Evidence-Centric Data Model

Replace the old `listings` schema. Runtime DB recreation is acceptable and preferred.

### Required Tables

#### `entities`

Canonical property entities.

Fields:

- `id TEXT PRIMARY KEY`
- `primary_provider TEXT NOT NULL`
- `primary_provider_id TEXT NOT NULL`
- `created_at TEXT NOT NULL`
- `updated_at TEXT NOT NULL`
- `status TEXT NOT NULL`

Unique:

- `(primary_provider, primary_provider_id)`

#### `entity_identifiers`

Provider and external IDs.

Fields:

- `entity_id`
- `provider`
- `identifier`
- `confidence`
- `source_snapshot_id`
- `created_at`

Examples:

- `rightmove:170448131`
- `uprn:100023456789`
- `epc:lmk-key`

#### `source_snapshots`

Raw or semi-raw source payloads.

Fields:

- `id`
- `entity_id`
- `provider`
- `source_kind`
- `request_url`
- `request_params_json`
- `status_code`
- `content_type`
- `body_path`
- `body_json`
- `body_hash`
- `fetched_at`
- `expires_at`
- `error_code`
- `error_message`

Body storage:

- Store large HTML/images on disk under cache.
- Store JSON payloads in DB if reasonably small.
- Always store hash and path.

#### `observations`

Extracted source-specific facts before canonical resolution.

Fields:

- `id`
- `entity_id`
- `source_snapshot_id`
- `provider`
- `field`
- `value_json`
- `unit`
- `source_path`
- `confidence`
- `observed_at`

Examples:

- Rightmove `address.displayAddress`
- Rightmove `location.latitude`
- Rightmove `keyFeatures[2]`
- EPC `floorAreaSqm`
- Mapbox candidate coordinate

#### `address_candidates`

Structured address candidates from all sources.

Fields:

- `id`
- `entity_id`
- `source`
- `address_text`
- `postcode`
- `uprn`
- `lat`
- `lng`
- `pin_type`
- `match_score`
- `confidence`
- `unit_ambiguous`
- `reasons_json`
- `source_snapshot_id`
- `created_at`

#### `address_resolutions`

Selected canonical resolution.

Fields:

- `entity_id PRIMARY KEY`
- `selected_candidate_id`
- `confidence`
- `status`
- `conflicts_json`
- `fallback_path_json`
- `resolved_at`

#### `corrections`

Append-only manual or agent-supplied corrections.

Fields:

- `id`
- `entity_id`
- `kind`
- `payload_json`
- `source`
- `author`
- `note`
- `active`
- `supersedes_correction_id`
- `affected_sections_json`
- `created_at`

Kinds:

- `address`
- `epc`
- `media`
- `fact`

Rules:

- Corrections are evidence, not mutations of source snapshots.
- Active corrections can influence selected address, selected EPC certificate, map coordinates, and dependent facts.
- Inactive corrections remain queryable for audit history.
- Evidence bundles should include active correction ids and indicate when a section used a correction.

#### `facts`

Source-backed facts for agent assessment.

Fields:

- `id`
- `entity_id`
- `category`
- `field`
- `value_json`
- `unit`
- `source`
- `source_record_ref`
- `confidence`
- `observed_at`
- `stale_after`

Categories:

- `property`
- `address`
- `broadband`
- `epc`
- `transport`
- `deprivation`
- `census`
- `population`
- `income`
- `flood`
- `crime`
- `media`

#### `claims`

Claims extracted from listing text.

Fields:

- `id`
- `entity_id`
- `source`
- `claim_type`
- `raw_text`
- `normalized_text`
- `source_span_json`
- `confidence`
- `created_at`

Examples:

- `broadband.gigabit_available`
- `parking.available`
- `garden.private`
- `epc.rating`
- `transport.near_station`

#### `verifications`

Claim-vs-fact outcomes.

Fields:

- `id`
- `entity_id`
- `claim_id`
- `verification_type`
- `result`
- `confidence`
- `facts_json`
- `reasoning`
- `created_at`

`result` values:

- `supported`
- `contradicted`
- `unknown`
- `insufficientEvidence`

#### `media_assets`

Media manifest and local cache records.

Fields:

- `id`
- `entity_id`
- `kind`
- `remote_url`
- `local_path`
- `relative_path`
- `content_hash`
- `width`
- `height`
- `mime_type`
- `status`
- `normalization_profile`
- `error_code`
- `failure_reason`
- `source_snapshot_id`
- `correction_id`
- `created_at`

Kinds:

- `photo`
- `floorplan`
- `epc`
- `mapSatellite`
- `mapStreet`

#### `evidence_bundles`

Materialized bundle metadata.

Fields:

- `id`
- `entity_id`
- `depth`
- `sections_json`
- `summary_json`
- `status`
- `generated_at`
- `source_snapshot_ids_json`

#### `assessments`

Agent-authored assessment.

Fields:

- `id`
- `entity_id`
- `evidence_bundle_id`
- `assessment_json`
- `score`
- `recommendation`
- `created_at`

### Source DBs

Keep source DBs separate under `$LET_HOME/sources`:

- `broadband.db`
- `postcodes.db`
- `deprivation.db`
- `census.db`
- `population.db`
- `income.db`
- `flood.db`
- `naptan.db`
- `uprn.db`
- `crime.db`

Do not merge source DB tables into the runtime evidence DB.

## 9. Source Provider Requirements

### 9.1 Rightmove Provider

Reuse:

- PAGE_MODEL extraction.
- Listing status classification.
- Price parsing.
- Station extraction.
- Image/floorplan/EPC remote URL extraction.
- API and HTML search fallback.

Replace:

- Direct transform into `Listing`.
- Lowercase-only sanitized description.
- Single `description` string.
- Immediate canonical field assignment.

New outputs:

- Raw HTML snapshot.
- PAGE_MODEL JSON snapshot.
- `propertyData` observation set.
- `analyticsInfo` observation set.
- Description object:

```json
{
  "rawHtml": "...",
  "rawText": "...",
  "plainText": "...",
  "normalizedText": "...",
  "keyFeatures": [],
  "sourcePaths": []
}
```

Extraction should aim to preserve all useful Rightmove fields, not only currently scored fields.

### 9.2 Broadband Provider

Reuse:

- Ofcom source builder.
- Existing postcode, outward, area, LAUA, take-up, national summary tables.

New outputs:

- Full postcode broadband record.
- Outward aggregate.
- Area aggregate.
- LAUA coverage.
- Full-fibre take-up.
- National summary fallback, if local detail missing.

Claim verification:

- If description says "gigabit", "gigabit broadband", "full fibre", or equivalent, compare against postcode `gigabit_availability`, `pct_over_300mbps`, `ufbb_100_availability`, and `full_fibre_availability` where available.
- Use thresholds:
  - `supported`: high local availability, default >= 80 percent.
  - `contradicted`: low local availability, default <= 20 percent.
  - `unknown`: middle range or missing data.
  - `insufficientEvidence`: source DB missing or postcode unresolved.
- Record thresholds in config.

### 9.3 Address Provider

Inputs:

- Rightmove display address.
- Rightmove outcode/incode.
- Rightmove coordinates and pin type.
- EPC candidates.
- Mapbox candidates.
- Postcode DB.
- OS Open UPRN nearby candidates.
- User overrides.

Resolution priority:

1. User override with valid postcode/coordinates.
2. EPC candidate with strong address and UPRN match.
3. Rightmove accurate point plus unique nearby UPRN.
4. Mapbox address candidate consistent with postcode.
5. Postcode DB centroid.
6. Original Rightmove coordinates as low-confidence fallback.

Multiple flats:

- If candidates share a building but not unit, mark `unitAmbiguous: true`.
- Do not claim exact UPRN unless unit-level evidence supports it.

Wrong postcode:

- Preserve Rightmove postcode as an observation.
- Add corrected postcode as candidate.
- Mark conflict in address section.
- Normalize postcodes consistently before all local DB lookups. The live smoke for `CT21 5QR` showed broadband lookup succeeding while address postcode lookup still warned that the postcode was missing, so postcode normalization must be audited across `SourceEnricher`, address resolution, and postcode facts.
- If Rightmove provides only outcode/incode separately, join and normalize before lookup.
- If the user supplies a corrected address or postcode, rerun postcode facts, broadband, UPRN, EPC search, map media, and claim verification against the corrected candidate while keeping the Rightmove candidate visible.

Wrong EPC links:

- Treat Rightmove EPC image/link as evidence, not truth.
- Search EPC by normalized address/postcode.
- If linked EPC conflicts with best EPC candidate, expose both.

Manual correction flow:

- `correct address` should create or activate an address candidate with `source = manualCorrection`.
- A manual address correction can be text-only, postcode-only, coordinate-only, or a complete exact address.
- Text-only corrections should trigger Mapbox/EPC/UPRN lookup.
- Coordinate-only corrections should improve map/media and nearby UPRN search, but must not imply an exact postal address.
- If the corrected address changes the postcode, all postcode-derived facts must show which postcode they used.

### 9.4 EPC Provider

Reuse:

- Modern bearer-token API.
- Legacy basic auth fallback.
- Address-scoped and postcode-scoped searches.
- Certificate detail fetch.
- Address match scoring.

New outputs:

- Search requests and responses.
- Candidate list.
- Selected certificate.
- Match score.
- Ambiguity reasons.
- UPRN evidence.
- Floor area, rating, lodgement date, matched address.

Do not collapse candidate ambiguity into one nullable field.

Selection requirements:

- Store all EPC candidates returned by exact address, postcode, UPRN, and manual certificate lookups.
- Rank candidates by exact unit match, postcode match, UPRN match, address token score, certificate recency, and distance from selected coordinates where available.
- Treat Rightmove-linked EPC media as a source candidate, not automatically as the selected certificate.
- Allow `correct epc --certificate-url`, `correct epc --lmk-key`, and `correct epc --uprn` to pin the selected certificate.
- When a manual EPC correction is active, expose that fact in `evidence.epc.selected` and in any facts derived from EPC data.
- If the selected EPC rating or floor area conflicts with Rightmove text or agent description, emit a verification conflict rather than silently preferring EPC.
- If EPC auth is unavailable, keep Rightmove EPC media and candidate search status as `degraded` instead of failing `inspect`.

### 9.5 Media Provider

Reuse:

- Cache naming, hashes, normalization versioning.
- Photo/floorplan/EPC/map normalization.
- Download and process concurrency.
- Existing map image support.

New outputs:

- Media manifest section.
- Per-asset status.
- Dimensions, hash, local absolute path, relative cache path, remote URL.
- Failure reasons.

Media should be included in `inspect` as a manifest in `quick` mode and downloaded or refreshed according to depth/config.

Implementation requirements:

- Move the useful parts of `crates/let-sdk/src/pipeline/fetch/media.rs`, `cache.rs`, and `maps.rs` behind an intelligence media service instead of calling them through the old listing pipeline.
- Convert `RightmovePropertyExtract` media URLs into downloadable `MediaAssetInput` records with stable ids, kind, source snapshot id, preferred remote URL, fallback URLs, and caption.
- Download and normalize photos, floorplans, EPC graphs, and static maps into `$LET_HOME/cache/<rightmove-id-or-entity-id>/`.
- Persist every asset to `media_assets` with `remoteUrl`, `localPath`, `relativePath`, `contentHash`, `width`, `height`, `mimeType`, `status`, `failureReason`, and `normalizationProfile`.
- Keep partial media failure section-scoped. A failed floorplan download must not fail Rightmove capture or broadband verification.
- Respect config for map download, floorplan download, EPC asset download, target dimensions, JPEG quality, timeout, retry count, and concurrency.
- Expose enough media state for `let-tui` to render local images without re-scraping Rightmove.
- Add one smoke listing with photos and EPC media, then assert `inspect --depth standard` produces at least one local photo path when media download is enabled.

Recommended depth policy:

- `quick`: extract remote media manifest only, no downloads.
- `standard`: download and normalize photos plus maps by default; floorplan and EPC asset follow config.
- `deep`: refresh all stale media, include floorplans, EPC assets, map views, and failed retry attempts.

### 9.6 Local Fact Providers

Convert `SourceEnricher` into source-specific fact providers:

- `PostcodesFactProvider`
- `BroadbandFactProvider`
- `DeprivationFactProvider`
- `CensusFactProvider`
- `PopulationFactProvider`
- `IncomeFactProvider`
- `FloodFactProvider`
- `CrimeFactProvider`
- `NaptanFactProvider`
- `UprnFactProvider`

Each provider returns:

- `status`
- `facts[]`
- `sourceRecords[]`
- `warnings[]`
- `unavailableReason`

No provider should mutate the canonical entity directly.

## 10. New SDK Module Layout

Proposed layout:

```text
crates/let-sdk/src/
+-- intelligence/
|   +-- mod.rs
|   +-- inspect.rs
|   +-- evidence.rs
|   +-- status.rs
|   +-- schema.rs
|   +-- repository.rs
|   +-- capture/
|   |   +-- mod.rs
|   |   +-- rightmove.rs
|   |   +-- epc.rs
|   |   +-- mapbox.rs
|   |   +-- media.rs
|   +-- extract/
|   |   +-- mod.rs
|   |   +-- rightmove.rs
|   |   +-- claims.rs
|   +-- resolve/
|   |   +-- mod.rs
|   |   +-- address.rs
|   |   +-- epc.rs
|   |   +-- identity.rs
|   +-- facts/
|   |   +-- mod.rs
|   |   +-- broadband.rs
|   |   +-- postcodes.rs
|   |   +-- deprivation.rs
|   |   +-- census.rs
|   |   +-- population.rs
|   |   +-- income.rs
|   |   +-- flood.rs
|   |   +-- crime.rs
|   |   +-- naptan.rs
|   |   +-- uprn.rs
|   +-- verify/
|       +-- mod.rs
|       +-- broadband.rs
|       +-- epc.rs
|       +-- address.rs
|       +-- media.rs
+-- sources/
+-- paths.rs
+-- config.rs
+-- errors.rs
+-- utils/
```

CLI layout:

```text
crates/let-cli/src/commands/
+-- inspect.rs
+-- evidence.rs
+-- verify.rs
+-- assess.rs
+-- search.rs
+-- sources.rs
+-- health.rs
+-- config.rs
+-- tools.rs
```

Remove or rewrite:

- `fetch.rs`
- `view.rs`
- `score.rs` as public command
- `ops.rs`
- `export.rs`
- `build.rs` public shape

## 11. JSON Schema And Tool Registry Requirements

The registry must include non-null schemas for every structured command.

Recommended approach:

- Add `schemars` or hand-authored JSON schema constants.
- Prefer deriving schemas from Rust types where practical.
- Store tool metadata in one registry source.
- Add tests that fail if any public tool has null `inputSchema` or `outputSchema`, except group/help placeholders if those are still registered.
- Consider not registering group placeholders at all; only register runnable tools.

Minimum registry metadata for each command:

- `name`
- `command`
- `category`
- `description`
- `parameters`
- `outputFields`
- `inputSchema`
- `outputSchema`
- `idempotent`
- `rateLimit`
- `example`

`inspect` should explicitly state it mutates cache/DB.

`evidence` should explicitly state read-only behavior.

## 12. Error And Status Semantics

Keep central envelope:

```json
{ "ok": true, "data": {}, "meta": {} }
{ "ok": false, "error": { "code": "...", "message": "...", "hint": "..." }, "meta": {} }
```

Refine exit codes:

- `0`: command produced the requested envelope, including partial evidence bundles.
- `1`: validation, parse, network, source, or business failure where command could not produce requested output.
- `2`: blocked prerequisite such as missing config, unwritable dirs, DB schema mismatch, or required source DB unavailable with no fallback.

Recommended error codes:

- `invalid_input`
- `not_found`
- `source_unavailable`
- `source_rate_limited`
- `source_parse_error`
- `snapshot_missing`
- `evidence_missing`
- `schema_mismatch`
- `config_missing`
- `credential_missing`
- `permission_denied`
- `blocked_prerequisite`
- `internal_error`

Partial section failures should usually be represented inside a successful evidence bundle rather than as top-level command failure.

## 13. Configuration Changes

Refactor config away from `fetch` naming.

Current state:

- The production config at `/Users/han/.tools/let/data/let.config.toml` still uses `[search]`, `[search.filters]`, `[fetch]`, and `[scoring]`.
- The parser in `crates/let-sdk/src/config.rs` still accepts the full old `FetchConfig`, including `minScore`, `dropNewBelowMinScore`, media dimensions, media quality, media concurrency, map download flags, floorplan download, EPC asset download, and timeout.
- `search discover` still uses the search config and selected fetch throttling/list limits.
- New `inspect` currently loads `FetchConfig`, but the intelligence service does not yet honor most fetch media settings and does not apply scoring settings.
- Because this rebuild explicitly does not need legacy compatibility code, it is acceptable to migrate the config shape and update `/Users/han/.tools/let/data/let.config.toml` after the new parser and template are ready.

New config sections:

```toml
[search]
useApi = true

[inspect]
defaultDepth = "standard"
refreshPolicy = "stale"
staleAfterHours = 24
maxRetries = 3
delayMs = 250

[media]
downloadInStandard = false
downloadInDeep = true
downloadMaps = true
downloadFloorplan = true
downloadEpcAsset = true
photoLandscapeWidth = 1200
photoLandscapeHeight = 900
photoPortraitWidth = 900
photoPortraitHeight = 1200
qualityPhoto = 82

[address]
mapboxLimit = 5
uprnMaxDistanceM = 25
requireExactUnitForExact = true

[verification.broadband]
supportedGigabitThreshold = 80
contradictedGigabitThreshold = 20

[corrections]
allowAgentCorrections = true
requireNote = true

[tui]
defaultView = "listings"
openImageCommand = "qlmanage"

[score]
enabled = false
```

Environment variables:

- Keep `EPC_API_BEARER_TOKEN`.
- Keep legacy `EPC_API_EMAIL` and `EPC_API_KEY` only as fallback.
- Keep `MAPBOX_ACCESS_TOKEN`.
- Keep source override env vars.
- Remove or ignore `fetch.useApi`; keep `search.useApi`.

Update `.env.example` and templates.

Migration plan:

1. Add the new config structs and defaults in `crates/let-sdk/src/config.rs`.
2. Update `let config show` so agents can see the effective config, the config path, and any deprecated fields that were ignored.
3. Update the runtime template and `/Users/han/.tools/let/data/let.config.toml` only after tests prove the new shape works.
4. Delete old scoring and fetch config fields from the parser once equivalent active settings exist under `[inspect]`, `[media]`, `[address]`, `[verification]`, `[corrections]`, and `[tui]`.
5. Keep `search.useApi`, but keep rejecting `fetch.useApi`; discovery transport and listing detail capture must remain separate concepts.

## 14. Implementation Roadmap From Current Worktree

This roadmap assumes the current refactor stays in place. The next agent should work through the phases in order, but can split each phase into reviewable commits.

Priority order:

1. Stabilize the current slice and keep the build green.
2. Add correction primitives and dependency refresh.
3. Finish address/EPC reliability.
4. Finish config migration.
5. Finish TUI/media viewing.
6. Expand verifiers and optional scoring only after the evidence layer is reliable.

Do not add more public commands unless a real workflow cannot be expressed with `inspect`, `evidence`, `verify`, `correct`, `assess`, `sources`, `search`, `start`, `tools`, `health`, and `config show`.

### Roadmap Status

| Phase | Status | Main files | Completion bar |
| --- | --- | --- | --- |
| 0. Command surface | Mostly done | `crates/let-cli/src/main.rs`, `registry.rs`, `commands/` | Old commands unsupported, new commands registered, schemas non-null |
| 1. Evidence schema | Partly done | `crates/let-sdk/src/intelligence/repository.rs`, `types.rs`, `db/schema.sql` | Corrections and richer observations persisted; schema tests cover all tables |
| 2. Rightmove capture/extract | Partly done | `pipeline/fetch/rightmove.rs`, `intelligence/service.rs` | Raw description and source paths preserved; fixture coverage expanded |
| 3. Address resolution | Basic only | `intelligence/service.rs`, `pipeline/geocode.rs`, `pipeline/uprn.rs`, `pipeline/enrich.rs` | Candidate list, conflicts, postcode normalization, UPRN ambiguity, Mapbox fallbacks |
| 4. Fact providers | Basic join exists | `pipeline/enrich.rs`, `sources/`, future `intelligence/facts/` | Independent provider results with status, facts, warnings, source refs |
| 5. Claims/verification | Broadband only | `intelligence/service.rs`, future `intelligence/verify/` | Broadband fixtures plus EPC/address/media/description verifiers |
| 6. Media manifest | Photos/maps working | `pipeline/fetch/media.rs`, `cache.rs`, `maps.rs`, `intelligence/service.rs` | Per-asset persistence, PDF-aware EPC/floorplan handling, TUI projection |
| 6A. Corrections | Working slice landed | new CLI command + repository/service APIs | Expand dependency refresh and correction-aware verification explanations |
| 6B. Config migration | Not started | `config.rs`, templates, `config show` | `[fetch]` removed, `[inspect]` and `[media]` active, local config updated |
| 7. Inspect orchestration | Working slice | `intelligence/service.rs`, `commands/inspect.rs` | Section services separated and partial failures isolated |
| 8. Evidence/verify | Working slice | `commands/evidence.rs`, `commands/verify.rs`, repository | Read-only evidence proven; verify refresh scoped to selected claim |
| 9. Assessments | Working slice | `commands/agent_assess.rs`, repository | Assessment schema linked to evidence bundle and documented |
| 10. Score | Deferred | `pipeline/score.rs` | Optional evidence only, disabled by default |
| 11. TUI/start | Evidence-bundle loader started | `commands/start.rs`, `let-tui/src/app.rs` | Finish section UI and remote-only media states |

### Phase 0: Freeze Contract And Delete Legacy Surface

Tasks:

- Confirm the public command list remains the one in section 4.
- Keep `main.rs` command enum aligned with that list.
- Keep public `fetch`, `view`, `score`, `ops`, `export`, and old `build` commands removed.
- Add `correct` to the public surface.
- Keep `tools`, `health`, `config show`, `search`, `inspect`, `evidence`, `verify`, `assess`, `sources`, and `start`.
- Register only runnable commands.
- Keep registry metadata and schemas generated from one source.
- Add a regression test that fails when any runnable public tool has a null `inputSchema` or `outputSchema`.

Acceptance:

- `let tools` returns only target commands.
- No `inputSchema` or `outputSchema` is null for runnable tools.
- Unknown old commands return `unsupported_command`.
- `let start` is present and is installed by `bun run build`.
- `let correct` appears only after it is implemented end-to-end.

### Phase 1: Evidence Schema And Repository

Tasks:

- Finish the evidence-centric schema already introduced.
- Add missing `corrections` persistence.
- Add richer `observations` or equivalent source-observation rows if not already fully represented.
- Add repository functions for entities, identifiers, snapshots, observations, facts, address candidates, address resolutions, corrections, claims, verifications, media assets, bundles, and assessments.
- Make repository APIs return typed structs rather than loose JSON where the schema is known.
- Keep runtime DB recreation as the supported repair path for schema mismatches.
- Do not hand-migrate old listing DBs unless a later user requirement explicitly asks for migration.

Acceptance:

- DB initializes cleanly.
- Schema version is explicit and health reports mismatches as blocked.
- Tests cover each new table insert/read path.
- `health` recognizes new schema and gives correct recreation hint.
- `let inspect <id>` can recreate the DB from empty `$LET_HOME/data`.

### Phase 2: Rightmove Capture And Extraction

Tasks:

- Split current `rightmove.rs` into capture and extraction modules.
- Store raw HTML and PAGE_MODEL snapshot.
- Extract all Rightmove observations without collapsing into `Listing`.
- Preserve description raw/HTML/plain/normalized/keyFeatures.
- Preserve media remote URLs.
- Preserve status classification.

Acceptance:

- Fixture tests cover active, let-agreed, removed, missing PAGE_MODEL, and malformed PAGE_MODEL.
- Description tests prove raw content is preserved and normalized content is separately available.
- `let inspect <fixture-id> --section rightmove,description --refresh all` returns rightmove and description sections.

### Phase 3: Address Resolution

Tasks:

- Build `address_candidates` from Rightmove, postcode DB, Mapbox, EPC, UPRN.
- Expand Mapbox parsing to return candidate list, not only first coordinate.
- Store UPRN distance candidates.
- Implement resolution priority and fallback path.
- Mark ambiguity for multiple flats/building-level matches.
- Audit postcode normalization across Rightmove extraction, `SourceEnricher`, postcode facts, broadband lookup, UPRN lookup, EPC search, and address resolution.
- Fix the observed case where `CT21 5QR` can enrich broadband but address resolution still says the postcode is not found in the local postcode DB.
- Store all candidate reasons and conflicts in the evidence bundle.

Acceptance:

- Tests cover wrong Rightmove postcode corrected by Mapbox/EPC/postcode DB.
- Tests cover multiple UPRN candidates and ambiguous unit.
- Tests cover missing Mapbox token degrading to postcode or Rightmove fallback.
- Address section includes selected candidate, alternatives, conflicts, confidence, and fallback path.
- Postcode-derived sections report the exact postcode they used.
- A coordinate-only address correction improves map/UPRN resolution without claiming an exact postal address.

### Phase 4: Fact Providers

Tasks:

- Convert `SourceEnricher` into independent fact providers.
- Start with broadband, postcodes, EPC, UPRN, NaPTAN.
- Then add deprivation, census, population, income, flood, crime.
- Preserve source DB metadata in `sources status`.

Acceptance:

- Missing source DB degrades only that provider.
- `inspect --section facts` returns fact records with source, value, unit, confidence, and source record references.
- Broadband section includes postcode, outward, area, LAUA, take-up, and fallback details where available.

### Phase 5: Claims And Verification

Tasks:

- Implement deterministic claim extraction from description/key features.
- Start with broadband, EPC, parking/garden/pets/heating/station hints.
- Implement broadband verification thresholds.
- Implement EPC verification against candidate match.
- Implement address verification conflicts.
- Keep verification refresh scoped by claim. Do not regress into the old behavior where broadband verification refreshes media.
- Add tests for `sections_for_claim` or equivalent section-planning logic whenever a claim type is added.

Acceptance:

- Fixture: description says gigabit and broadband DB supports it.
- Fixture: description says gigabit and broadband DB contradicts it.
- Fixture: source DB missing returns `insufficientEvidence`.
- Verification records include claim ID, facts used, result, confidence, and reasoning.
- `verify --claim broadband --refresh all` does not download media or populate cache.
- `verify --claim media --refresh all` does include media.

### Phase 6: Media Manifest

Tasks:

- Adapt media normalization to work from observations and entity IDs rather than `Listing`.
- Store per-asset records.
- Return manifest in `inspect` and `evidence --section media`.
- Include local absolute path for agent image access and relative cache path for portability.
- Keep `localPath` present only when the file exists.
- Persist per-asset `status`, dimensions, hash, MIME type, normalization profile, and failure reason.
- Handle PDF EPC assets explicitly: either download as a PDF asset with `mimeType = application/pdf`, derive a preview image, or mark as remote with a clear reason.
- Ensure map assets are regenerated when corrected coordinates are active.

Acceptance:

- Media tests cover skip, cached, downloaded, failed, and normalized paths.
- Manifest includes dimensions and hash where available.
- Partial media failures do not fail entire inspect command.
- Missing cache files are reported as remote and do not count as cached.
- Production smoke for a listing with photos returns local photo paths and matching files under `$LET_HOME/cache`.

### Phase 6A: Corrections And Dependency Refresh

Tasks:

- Keep correction persistence and repository APIs.
- Keep `correct address`, `correct epc`, `correct media`, and `correct clear` registry-backed and envelope-safe.
- Keep active corrections visible in evidence bundles.
- Keep address, EPC, media, facts, and verification services consuming active corrections through explicit inputs.
- Build dependency refresh planning so address changes rerun postcode facts, broadband, UPRN, EPC, map media, and affected verifications.
- Ensure corrections can be disabled without deleting audit history.

Acceptance:

- A corrected postcode changes postcode-derived facts while preserving the original Rightmove postcode observation.
- A corrected EPC URL or LMK key pins the selected EPC certificate and marks conflicting candidates.
- Corrected map coordinates regenerate map media without redownloading unchanged photos.
- `verify` explains when a conclusion depends on a manual correction.
- Correction commands have registry schemas and emit one JSON envelope.
- Address/EPC/media correction smokes pass against a temp runtime.

### Phase 6B: Config Migration

Tasks:

- Replace old `[fetch]` and broad `[scoring]` runtime behavior with `[inspect]`, `[media]`, `[address]`, `[verification]`, `[corrections]`, `[tui]`, and optional `[score]`.
- Wire media settings into the intelligence media service.
- Wire address thresholds into address resolution.
- Wire broadband thresholds into claim verification.
- Wire TUI settings into `let start`.
- Update `.env.example`, `.claude/skills/let/templates/let.config.toml`, and `/Users/han/.tools/let/data/let.config.toml` after parser tests pass.
- Delete unused parser fields after the new config shape is active.

Acceptance:

- `let config show` exposes the effective new config shape.
- A config with invalid media dimensions or quality fails validation.
- Media download behavior changes when `[media]` settings change.
- Old unused scoring/fetch settings are not silently presented as active behavior.

### Phase 7: Inspect Orchestration

Tasks:

- Implement `InspectService`.
- Wire depth, refresh, and section filtering.
- Materialize evidence bundle.
- Generate summary and next actions.
- Ensure partial results are successful envelopes.

Acceptance:

- Agent can run one command to get a usable bundle.
- Failed EPC or Mapbox does not block Rightmove, facts, claims, and media sections.
- Output is stable JSON and Toon.
- Integration tests verify quick, standard, deep, and section-specific modes.

### Phase 8: Evidence And Verify Commands

Tasks:

- Implement read-only `evidence`.
- Implement targeted `verify`.
- Ensure `evidence` never mutates.
- Ensure `verify` mutates only selected verification/snapshot sections.

Acceptance:

- `evidence` on missing ID returns `not_found`.
- `evidence --section broadband` returns only requested section plus minimal metadata.
- `verify --claim broadband` refreshes verification without rerunning full inspect.

### Phase 9: Assessments

Tasks:

- Replace old assessment schema.
- Implement `assess save/get`.
- Require evidence bundle reference.
- Store assessment JSON as authored plus extracted top-level fields for filtering.

Acceptance:

- Invalid JSON returns structured validation errors.
- Assessment retrieval includes linked evidence bundle ID.
- Assessment does not trigger scoring or fact collection.

### Phase 10: Optional Scoring Evidence

Tasks:

- Decide whether deterministic score remains.
- If retained, make it a section named `score`.
- Compute from resolved facts, not raw listing row.
- Keep it disabled by default unless config enables it.

Acceptance:

- Agent sees deterministic score as context, not final recommendation.
- Missing factors reduce confidence rather than causing hidden zero defaults.

### Phase 11: Finish `start`, TUI, And External Sync Boundary

Tasks:

- Keep `let start` as a thin launcher for `let-tui`.
- Rebuild TUI data access on the evidence DB, not the old `listings` table.
- Add a projection layer from `EvidenceBundle`/repository rows to TUI view models.
- Show section statuses, warnings, source facts, claims, verifications, assessments, and active corrections.
- Restore image viewer behavior using local paths from `media_assets`.
- Show remote-only media as unavailable for local preview with a clear section warning.
- Add keyboard actions for opening local photos, floorplans, EPC assets, and maps.
- Make `let-tui` consume `LET_START_ID` and `LET_START_SECTIONS` or replace that env handoff with explicit CLI args passed by `let start`.
- Keep TUI read-only unless a later phase explicitly adds correction capture.
- Defer Notion export until evidence and assessment model stabilizes.

Acceptance:

- `let start` launches `let-tui` with the same runtime path flags as the CLI.
- `let start --id <id> --section media,address` opens the TUI focused on that listing and section set.
- `let-tui` compiles against the new repository.
- A listing inspected with media downloads enabled shows local photos in the TUI.
- A listing with remote-only media shows the manifest but does not crash the viewer.
- No Notion command remains unless it supports the new model and dry-run without credential validation side effects.

## 14A. Immediate Next Slices

These are the next practical commits from the current state.

### Slice A: Stabilize And Stage Current Refactor

Purpose:

- Make the current refactor coherent in git before adding more behavior.

Tasks:

- Stage the latest `verify` and media evidence fixes with the rest of the refactor.
- Inspect `git diff --cached` and `git diff` to ensure no duplicate staged/deleted file state remains.
- Re-run `bun run util:check`.
- Run production smoke commands against `/Users/han/.tools/let/let`.

Acceptance:

- `git status --short` shows a coherent staged set, not staged deletion plus unstaged replacement for the same file.
- `bun run util:check` passes.
- `verify --claim broadband --refresh all` leaves a fresh temp cache empty.
- `inspect --section media --refresh all` creates local photo cache files.

### Slice B: Add `correct` As The Missing Primitive

Purpose:

- Done as a working slice. Remaining work is richer dependency refresh and correction-aware verification explanations, not command existence.

Tasks:

- Keep `Correct` subcommands in `crates/let-cli/src/main.rs` and thin wrappers in `crates/let-cli/src/commands/correct.rs`.
- Keep registry entries and schemas for `correct.address`, `correct.epc`, `correct.media`, and `correct.clear`.
- Keep the additive `corrections` table and repository APIs.
- Keep active correction loading in the inspect service.
- Expand dependency refresh planning:
  - address correction reruns address, postcode facts, broadband, UPRN, EPC candidate search, map media, claims/verifications affected by address;
  - EPC correction reruns EPC facts and EPC verification;
  - media correction reruns only map media unless more media kinds are added.
- Keep active corrections visible in `evidence`.

Acceptance:

- `let correct address <id> --postcode <postcode> --note <text>` returns `correctionId`, affected sections, and next command suggestions.
- Re-running `inspect` after address correction shows original Rightmove address and corrected address as separate evidence.
- `let correct epc <id> --lmk-key <key> --note <text>` pins the selected EPC without deleting candidates.
- `let correct clear <id> --kind address --correction-id <id>` disables but does not delete the correction.

### Slice C: Address And EPC Reliability

Purpose:

- Make listing identity and dependent facts trustworthy enough for agent decisions.

Tasks:

- Normalize postcode handling in one helper and use it everywhere.
- Expand Mapbox capture to store candidate lists.
- Store EPC candidates and ambiguity reasons.
- Add UPRN candidate storage and unit-ambiguity flags.
- Make address resolution explain fallback path and conflicts.
- Add fixtures for wrong postcode, multiple flats, wrong EPC link, and missing credentials.

Acceptance:

- Wrong Rightmove postcode does not poison broadband, EPC, or UPRN facts when a correction or better source exists.
- Multiple flats in one building return `confidence: ambiguous` unless unit-level evidence is strong.
- EPC conflicts are visible in `evidence.epc`, not silently collapsed.

### Slice D: Config Migration

Purpose:

- Stop carrying an old `fetch` config shape into the new agent-native architecture.

Tasks:

- Add `[inspect]`, `[media]`, `[address]`, `[verification]`, `[corrections]`, `[tui]`, and optional `[score]` structs.
- Wire `[media]` into media download behavior.
- Wire `[verification.broadband]` thresholds into broadband verification.
- Wire `[address]` thresholds into resolution.
- Update templates and `.env.example`.
- Update `/Users/han/.tools/let/data/let.config.toml` after parser tests pass.
- Remove old parser fields that no longer affect behavior.

Acceptance:

- `let config show` exposes the new effective shape.
- Invalid media dimensions or quality fail validation.
- Changing `[media]` settings changes inspect media behavior.
- Old `[fetch]` keys are rejected or clearly reported as ignored; they are not silently accepted as active behavior.

### Slice E: TUI Evidence Projection

Purpose:

- Partially done. `let-tui` now loads evidence bundles and start focus input; remaining work is richer evidence UI and remote-only media states.

Tasks:

- Keep the evidence-bundle projection for TUI listing rows.
- Show entity, Rightmove summary, section statuses, warnings, facts, claims, verifications, and assessment.
- Keep loading local media paths from the bundle media section; later move to direct `media_assets` projection if needed.
- Keep `LET_START_ID` and `LET_START_SECTIONS` focus behavior covered by `let start`.
- Distinguish remote-only media from cached media in the UI.

Acceptance:

- `let start --id 89872542 --section media` opens focused on that listing after it has been inspected.
- Cached photos render in the image preview.
- Remote-only EPC PDF does not crash the viewer.
- TUI remains read-only.

## 15. Validation Plan

### Required Local Gate

Run:

```sh
bun run util:check
```

### Targeted Tests

CLI surface:

```sh
cargo test -p let-cli --test cli_integration
```

DB/repository:

```sh
cargo test -p let-sdk --test db_tests
```

Provider modules:

```sh
cargo test -p let-sdk rightmove
cargo test -p let-sdk epc
cargo test -p let-sdk geocode
cargo test -p let-sdk broadband
cargo test -p let-sdk uprn
cargo test -p let-sdk naptan
```

### Manual Smokes

Run with a temp `LET_HOME`:

```sh
LET_HOME=/tmp/let-rebuild-smoke cargo run -q -p let-cli -- tools
LET_HOME=/tmp/let-rebuild-smoke cargo run -q -p let-cli -- health
LET_HOME=/tmp/let-rebuild-smoke cargo run -q -p let-cli -- config show
LET_HOME=/tmp/let-rebuild-smoke cargo run -q -p let-cli -- sources list
LET_HOME=/tmp/let-rebuild-smoke cargo run -q -p let-cli -- sources build all --jobs 3 --progress plain
LET_HOME=/tmp/let-rebuild-smoke cargo run -q -p let-cli -- sources status
LET_HOME=/tmp/let-rebuild-smoke cargo run -q -p let-cli -- search resolve York
LET_HOME=/tmp/let-rebuild-smoke cargo run -q -p let-cli -- inspect 170448131 --depth quick
LET_HOME=/tmp/let-rebuild-smoke cargo run -q -p let-cli -- inspect 170448131 --depth standard --refresh all
LET_HOME=/tmp/let-rebuild-smoke cargo run -q -p let-cli -- evidence 170448131
LET_HOME=/tmp/let-rebuild-smoke cargo run -q -p let-cli -- verify 170448131 --claim broadband
LET_HOME=/tmp/let-rebuild-smoke cargo run -q -p let-cli -- correct address 170448131 --postcode YO1 7HH --note smoke
LET_HOME=/tmp/let-rebuild-smoke cargo run -q -p let-cli -- correct epc 170448131 --lmk-key SAMPLE --note smoke
LET_HOME=/tmp/let-rebuild-smoke cargo run -q -p let-cli -- start --id 170448131
```

### Contract Tests

Add tests proving:

- Every structured command emits exactly one JSON envelope on stdout.
- `--toon` decodes to the same envelope shape.
- No progress logs appear on stdout.
- Every runnable tool has non-null `inputSchema` and `outputSchema`.
- `health` returns `ready|degraded|blocked`.
- Blocked prerequisites use exit code `2`.
- Partial `inspect` uses exit code `0` with section-level failures.
- Read-only `evidence` does not mutate DB timestamps or files.

### Scenario Tests

Create fixtures for:

- Active Rightmove listing.
- Let-agreed listing.
- Removed listing.
- Missing PAGE_MODEL.
- Description with rich HTML formatting.
- Description that mentions gigabit and source DB supports it.
- Description that mentions gigabit and source DB contradicts it.
- Wrong Rightmove postcode.
- Ambiguous EPC candidates.
- Wrong Rightmove EPC link.
- Multiple flats in one building.
- Missing Mapbox token.
- Missing broadband DB.
- Media download partial failure.
- Media download success with at least one local photo path.
- Map media regeneration after corrected coordinates.
- Manual address correction changing postcode and rerunning postcode-derived facts.
- Manual EPC correction by URL or LMK key.
- TUI listing backed by the evidence DB.
- TUI image viewer for downloaded photos/maps.

## 16. Implementation Guardrails

- Keep CLI commands thin. Put logic in SDK services.
- Preserve one-envelope stdout.
- Put all logs, warnings, and progress on stderr.
- Do not let read-only commands mutate.
- Do not silently overwrite observations with enriched values.
- Do not hide conflicts. Return conflicts as first-class data.
- Do not make AI judgment a tool-side default.
- Do not add public commands for every internal provider.
- Do not keep legacy compatibility wrappers.
- Do not hand-edit runtime DBs or source DBs.

## 17. Proposed Acceptance Definition

The rebuild is complete when:

- `let inspect <rightmove-id> --depth standard` returns one holistic evidence bundle.
- The bundle includes Rightmove, description, address, facts, claims, verification, and media sections.
- Each section has status, confidence, provenance, and next actions.
- Broadband claims are verified against the broadband DB.
- Address resolution handles wrong postcode, ambiguous flats, EPC candidates, Mapbox candidates, UPRN candidates, and fallback coordinates.
- Manual corrections can override address, EPC, and map coordinates without destroying source evidence.
- `let start` launches the TUI against the evidence DB.
- TUI shows downloaded photos, floorplans, EPC assets, and maps from local media cache paths.
- Media config controls download behavior and `inspect` persists local paths when downloads are enabled.
- The command surface is small and discoverable through `let tools`.
- All public command schemas are present.
- Agents can assess a listing from `inspect` plus optional `evidence`/`verify`, without using internal provider commands.
- `bun run util:check` passes.

## 18. Handoff Order For The Next Agent

The next agent should not begin by redesigning the command surface again. The command decision is made. Start from the current working refactor and execute in this order:

1. **Stabilize current worktree**
   - Stage the current unstaged fixes.
   - Confirm there is no stale staged delete/re-add mismatch.
   - Run `bun run util:check`.
   - Run the broadband verify and media inspect production smokes.

2. **Implement `correct`**
   - This is the most important missing primitive because it restores the real-world workflow where Rightmove has the wrong postcode, EPC link, map pin, or flat-level address.
   - Keep corrections append-only and auditable.
   - Make dependent refresh explicit.

3. **Fix address resolution**
   - Centralize postcode normalization.
   - Store candidates from Rightmove, postcode DB, Mapbox, EPC, UPRN, and corrections.
   - Expose selected candidate, alternatives, conflicts, ambiguity, and fallback path.

4. **Fix EPC resolution**
   - Store candidates, selected certificate, match reasons, and ambiguity.
   - Let `correct epc` pin a certificate by URL, LMK key, or UPRN.
   - Verify Rightmove EPC links and description claims against the selected EPC certificate.

5. **Finish media/TUI loop**
   - Persist per-asset media records with exact status and failure reasons.
   - Handle floorplan and EPC PDF assets deliberately.
   - Make `let-tui` read from the evidence DB and show cached local media.

6. **Migrate config**
   - Replace old `[fetch]` semantics with `[inspect]` and `[media]`.
   - Keep `search.useApi`; continue rejecting `fetch.useApi`.
   - Update templates and the production config only after parser tests pass.

7. **Expand verification**
   - Add fixtures and verifiers for broadband, EPC, address, station proximity, garden/parking/pets/heating where source-backed.
   - Keep agent judgment outside the CLI.

8. **Only then revisit optional scoring**
   - If deterministic score remains, expose it as optional evidence.
   - Do not gate discovery, media download, or assessment on score.

Commit strategy:

- Commit the stabilized current refactor first.
- Then use one commit per slice where possible:
  - `feat(correct): add manual evidence corrections`
  - `fix(address): resolve postcode and candidate ambiguity`
  - `feat(epc): persist certificate candidates and overrides`
  - `feat(tui): render intelligence media`
  - `refactor(config): migrate inspect and media settings`

Do not combine config migration, correction semantics, and TUI projection in one commit. Those are separate risk surfaces.
