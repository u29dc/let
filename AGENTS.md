## 1. Documentation

- Purpose: run `let` as an agent-native property search toolbelt; compose primitives and keep decisions in the agent.
- Source priority: executable contracts first, then code, then prose.
- Runtime truth commands:
    - `:let tools --json`
    - `:let health --json`
    - `:let config show --json`
- JSON-mode contract: in `--json` mode, write exactly one envelope JSON object to stdout and write logs only to stderr.
- Agent skill entry: `.claude/skills/let/SKILL.md`.
- Core references:
    - Runtime: `https://bun.sh/docs/llms.txt`
    - Validation: `https://zod.dev/llms.txt`
    - EPC: `https://epc.opendatacommunities.org/docs/api`
    - Postcodes: `https://postcodes.io/docs`
    - Notion: `https://developers.notion.com/llms.txt`
    - Mapbox: `https://docs.mapbox.com/llms.txt`

## 2. Repository Structure

```text
.
├── $LET_HOME/let (compiled binary)
├── packages/
│   ├── cli/src/
│   │   ├── index.ts
│   │   ├── main.ts
│   │   ├── envelope.ts
│   │   ├── tool.ts
│   │   └── commands/{assess,config,export,fetch,health,ops,score,search,tools,view}
│   ├── cli/tests/
│   ├── core/src/
│   │   ├── paths.ts
│   │   ├── db/{index.ts,schema.sql}
│   │   ├── schema/
│   │   ├── pipeline/{fetch,parse,enrich,score,assess,view,output}
│   │   └── utils/
│   └── core/tests/
├── scripts/{build-sources.ts,sources/*}
└── $LET_HOME/{data,cache,sources}
```

- CLI command registry source of truth: `packages/cli/src/tool.ts`.
- CLI envelope contract source of truth: `packages/cli/src/envelope.ts`.
- Path resolution source of truth: `packages/core/src/paths.ts`.
- Domain schema source of truth: `packages/core/src/schema/*`, `packages/core/src/db/schema.sql`.

## 3. Stack

| Layer      | Choice                  | Notes                                  |
| ---------- | ----------------------- | -------------------------------------- |
| Runtime    | Bun                     | CLI runtime, fetch, SQLite integration |
| Language   | TypeScript              | strict mode with checked indexing      |
| Validation | Zod 4                   | runtime schemas + type inference       |
| CLI        | citty                   | nested command tree and typed args     |
| Storage    | SQLite                  | local listing DB + source DBs          |
| Config     | TOML                    | user config parsing                    |
| Quality    | Biome + tsgo + bun test | lint, types, tests                     |

- Optional environment keys: `EPC_API_KEY`, `NOTION_API_KEY`, `MAPBOX_ACCESS_TOKEN`.
- Storage root: `$LET_HOME` (defaults to `~/.tools/let`).

## 4. Commands

- Bootstrap: `bun install`.
- Agent entrypoint: `:let <command>` (shell alias for compiled binary).
- Dev entrypoint: `bun run let <command>` (Bun dev runtime; development only).
- Compiled binary build: `bun run build:cli`.
- Source DB build: `bun run build:sources`.
- Single-source build: `bun run build:source:{broadband|postcodes|deprivation|census|population|income|flood|naptan|uprn|crime}`.
- Full quality gate: `bun run util:check`.

- Infrastructure command `let tools`: capability discovery from `toolRegistry[]`; supports detail lookup by tool name.
- Infrastructure command `let health`: prerequisites, source DB checks, key checks, directory writability checks, and fix commands.

- Tool `let config show`: load and display parsed config.
- Tool `let config validate`: validate config and return structured errors.
- Tool `let search resolve <location>`: resolve text to Rightmove location identifiers.
- Tool `let search discover`: discover portal IDs for configured or overridden locations.
- Tool `let search diff <ids>`: classify IDs as new vs known.
- Tool `let fetch <ids>`: fetch, parse, enrich, score, and persist listings.
- Tool `let view list`: ranked shortlist view with filtering and sort controls.
- Tool `let view detail <id>`: full listing payload.
- Tool `let score compute`: recompute scores for all listings.
- Tool `let score explain <id>`: factor-level score and penalty breakdown.
- Tool `let assess candidates`: unassessed listings ranked by score.
- Tool `let assess context <id>`: assessment bundle including media paths and context fields.
- Tool `let assess submit <id> <json>`: persist assessment and adjusted score.
- Tool `let export json`: write DB snapshot as JSON.
- Tool `let export notion`: sync selected listings to Notion.
- Tool `let ops verify`: verify listing activity status on portal.
- Tool `let ops prune`: prune listings by region, score, or inactive status.

- Common flags: `--json`, `--data-dir`, `--config-dir`, `--cache-dir`, `--sources-dir`.
- Fetch/select flags: `--skip-images`, `--skip-epc`, `--top`, `--region`, `--min-score`, `--sort`, `--asc`.
- Mutating safety flag: `--dry-run` for verification, prune, and Notion export preview.

- Recommended agent loop:

```bash
:let tools --json
:let health --json
:let search discover --json
:let search diff <id1>,<id2>,<id3> --json
:let fetch <new-id> --json
:let view list --json
:let assess context <uuid> --json
:let assess submit <uuid> '<assessment-json>' --json
:let view detail <uuid> --json
:let export json --output /tmp/let-export.json --json
```

## 5. Architecture

- System shape: CLI-only monorepo with command orchestration in `packages/cli` and domain logic in `packages/core`.
- Pipeline contract: Fetch -> Parse -> Enrich -> Score -> Assess -> View -> Output.
- Command design rule: primitives only; combine commands in the agent, not in CLI batch workflows.
- Tool metadata contract: commands using `defineToolCommand()` auto-register in global `toolRegistry[]` for discoverability.
- Infrastructure commands `tools` and `health` are intentionally outside the registry.

- Envelope contract:
    - Success: `{ ok: true, data, meta: { tool, elapsed, count?, total?, hasMore? } }`
    - Error: `{ ok: false, error: { code, message, hint }, meta: { tool, elapsed } }`
    - Exit codes: `0` success, `1` runtime error, `2` blocked prerequisites.

- Path resolution precedence (high to low):
    1. CLI overrides (`--data-dir`, `--config-dir`, `--cache-dir`, `--sources-dir`)
    2. Env overrides (`LET_DATA_DIR`, `LET_CONFIG_DIR`, `LET_CACHE_DIR`, `LET_SOURCES_DIR`)
    3. `LET_HOME` or `TOOLS_HOME` env var, defaulting to `~/.tools/let`

- Data model contract:
    - Listings carry internal UUID `id` plus portal IDs under `portalIds.rightmove`.
    - Status lifecycle is `active|inactive`; verify command updates status.
    - Scores include composites (`affordability|location|liveability`) and penalties (`epc|garden|pets`).
    - Assessments are persisted payloads; later submission overwrites previous assessment.

- Storage contract:
    - Primary DB: `$LET_HOME/data/let.db`
    - Backup DB: `$LET_HOME/data/let.db.bak`
    - JSON export: `$LET_HOME/data/let.db.json` or explicit `--output`
    - Cache: `$LET_HOME/cache/{portalId}/data.json` and media artifacts
    - Source DBs: `$LET_HOME/sources/{postcodes,broadband,deprivation,census,population,income,flood,crime,naptan,uprn}.db`

- Enrichment and scraping contract:
    - Search extraction from `__NEXT_DATA__`.
    - Listing extraction from `window.PAGE_MODEL`.
    - EPC, broadband, area metrics, and map snapshots are enrichment stages.
    - HTML fallback path exists when API paths are blocked.

- Operational constraints:
    - Rightmove requires request spacing; default behavior expects delay to avoid 429.
    - Notion writes are rate-limited around 3 req/s.
    - Source dataset builds are heavyweight and may degrade when upstream URLs expire.

## 6. Quality

- Required completion gates:
    - zero type errors
    - zero linter warnings
    - passing tests
    - successful CLI build (`bun run build:cli`)

- Standard checks:
    - `bun run util:format`
    - `bun run util:lint`
    - `bun run util:types`
    - `bun test`
    - `bun run util:check`

- Test surface:
    - CLI: envelope, contract, parity-gate, fetch-partial, binary-smoke, health.
    - Core: parse, sanitize, scraper, score, maps, EPC, source-schema, integration paths.

- Manual live E2E flow:

```bash
bun run build:cli
:let tools --json
:let health --json
:let config show --json
:let search discover --json
:let search diff <id1>,<id2>,<id3> --json
:let fetch <new-id> --skip-images --json
:let view list --json
:let view detail <uuid> --json
:let score explain <uuid> --json
:let export json --output /tmp/test-export.json --json
```

- Known risks to surface in agent output:
    - source DB availability or staleness
    - portal throttling and partial fetches
    - schema incompatibility requiring DB recreation or restore
