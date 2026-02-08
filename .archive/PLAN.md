# Agent-Native Re-Architecture of `let`

## Context

The `let` CLI scrapes Rightmove rental listings, enriches them with 10 local datasets (EPC, broadband, crime, deprivation, census, population, income, flood, UPRN, postcodes), and scores them via a variance-adaptive scoring engine. The core pipeline works well, but the CLI surface is human-workflow-shaped: `fetch batch` runs the entire pipeline as a monolithic operation, `view list` prints terminal tables, and AI assessment is bolted on via Claude Code skills (`/assess`, `/summarize`) that the human must invoke manually.

This refactor replaces the CLI orchestration layer with atomic, agent-composable primitives. A single `/let` skill launches an autonomous agent that uses the CLI binary as a toolbelt: discovers listings, fetches selectively, triages, deep-dives, researches neighborhoods, writes assessments, and produces a final report. No human sequencing required.

The core domain internals (fetch, parse, enrich, score, assess, DB, config, schemas) are preserved. Changes are to orchestration, CLI surface, output contracts, and path resolution only.

---

## Plan Synthesis

### From Plan A (snug-napping-flame)

**Kept**: JSON envelope `{ ok, data, meta }`; `--json` flag (simpler than `--format`); citty framework retained; pragmatic phased approach; error codes with `hint` field; `fetch search` as discovery primitive; assessment `--data` rename to free `--json` for output.

**Dropped**: `fetch batch` retained as monolithic (violates granularity); `view status` too limited for health; no tool discovery; no cross-platform paths; `view stats`/`view regions` over-complicating list.

### From Plan B (swirling-exploring-owl)

**Kept**: Tool registry with capability discovery; comprehensive health check with remediation; cross-platform path resolution; assessment context bundle; score explain; `search diff` as separate primitive; source DB health registry; stateless design.

**Dropped**: Custom arg parser (fragile); 28+ tools (too many for MVP); `entity.action` naming; removes `console-table-printer` (unnecessary rewrite); `listing update/delete/count`, `ops cache/backup`, `schema` (not MVP).

---

## Assumptions

1. **Bun** remains the runtime (no Node.js compat needed).
2. **Compiled binary** (`bin/let`) continues to be the distribution format.
3. **Source builders** (`sources/builders/`) remain dev-time scripts run from the monorepo; not part of the agent-native tool surface.
4. **Agent consumer** is Claude Code running in a terminal.
5. **No web UI or API server** -- CLI-only interface.
6. **Monorepo structure** (`packages/core`, `packages/cli`) is preserved.
7. **Citty** and **console-table-printer** are retained as dependencies.
8. **Cross-platform** means macOS + Linux only (no Windows).
9. **Existing tests** in `packages/core/tests/` continue to pass without modification.
10. **Assessment schema** is unchanged (same fields, same validation).

## Decision Points

| Decision                         | Recommended                                        | Rationale                                                           |
| -------------------------------- | -------------------------------------------------- | ------------------------------------------------------------------- |
| Keep citty vs custom parser      | Keep citty                                         | Lower risk, proven routing, overnight timeline                      |
| `--json` vs `--format`           | `--json` flag                                      | Simpler, standard pattern, less typing                              |
| JSON default vs text default     | Text default, `--json` opt-in                      | Agent always passes `--json`; humans get readable output by default |
| Tool registry source of truth    | `defineToolCommand()` wrapper enriching citty defs | One definition per command, no parallel file, no drift              |
| Path resolution location         | `packages/core/src/paths.ts` (shared)              | Used by both CLI and core enrichment code                           |
| `fetch batch` retained?          | No                                                 | Agent composes `search discover` + `search diff` + `fetch <ids>`    |
| Source builders in tool surface? | No (later phase)                                   | Not needed for agent loop                                           |

---

## Storage Model

### Database Files

| File                      | Purpose                                      | Owner                        | Schema Source                     |
| ------------------------- | -------------------------------------------- | ---------------------------- | --------------------------------- |
| `let.db`                  | Listings + scores + assessments + metadata   | `@let/core/db`               | `packages/core/src/db/schema.sql` |
| `let.db.bak`              | Automatic backup (created on each save)      | `@let/core/db`               | Same schema                       |
| `let.db.json`             | JSON export (generated by `export json`)     | CLI command                  | Serialized `ListingsFile`         |
| `{name}.db` (sources dir) | Source datasets (broadband, postcodes, etc.) | `sources/builders/{name}.ts` | Per-builder DDL                   |

### Schema Ownership

- **DDL**: `packages/core/src/db/schema.sql` is the single schema definition. `openListingsDb()` runs `CREATE TABLE IF NOT EXISTS` from this file on every open.
- **No migration system**: Schema changes require manual intervention or DB recreation. The health check detects schema incompatibility by attempting to load a listing and catching column errors.
- **Read/write API**: `loadListingsFile()` and `saveListingsFile()` in `@let/core/db/index.ts` own all DB I/O. Commands never execute raw SQL.
- **Source DBs**: Each builder defines its own schema inline. Source DBs are read-only from the perspective of CLI commands (enrichment reads, never writes).

### Schema Compatibility Check

The `health` command verifies DB compatibility by:

1. Checking `let.db` exists (info-level if missing -- created on first fetch)
2. Attempting `loadListingsFile()` with a catch for column errors
3. On schema mismatch: severity `blocking`, hint includes backup recovery command

---

## Target Architecture

```
Agent (Claude Code, /let skill)
  |  discovers tools via `let tools --json`
  |  checks prereqs via `let health --json`
  |  invokes tools via `let <prefix> <action> [args] --json`
  |  reads structured JSON from stdout only
  |  stderr is for logs/progress (ignored by agent)
  v
CLI Dispatcher (packages/cli/src/index.ts)
  |  citty root command with 10 subcommand groups
  |  each group has 1-3 subcommands defined via defineToolCommand()
  |  --json flag triggers JSON envelope output
  v
Command Handlers (packages/cli/src/commands/<prefix>/<action>.ts)
  |  validate input -> resolve paths -> call core -> format output
  |  no command calls another command; composition is agent-level
  v
Shared Infrastructure
  |  paths.ts (core): path resolution used by CLI + core enrichment
  |  envelope.ts (cli): JSON envelope write to stdout
  |  defineToolCommand() (cli): citty wrapper that registers tool metadata
  v
Core Library (@let/core) -- UNCHANGED
  pipeline/{fetch,parse,enrich,score,assess,view,output}
  schema/, db/, config/, utils/
```

### Directory Structure (Final State)

```
packages/core/src/
  paths.ts                          # NEW: cross-platform path resolution (shared)
  ...                               # everything else unchanged

packages/cli/src/
  index.ts                          # citty root: 10 subcommand groups
  envelope.ts                       # JSON envelope (ok/fail/isJsonMode)
  tool.ts                           # defineToolCommand() wrapper + registry array
  commands/
    assess/
      candidates.ts                 # list unassessed listings ranked by score
      context.ts                    # context bundle for AI assessment
      index.ts                      # citty group: candidates, context, submit
      submit.ts                     # write assessment back
    config/
      index.ts                      # citty group: show, validate
      show.ts                       # show parsed configuration
      validate.ts                   # validate config file
    export/
      index.ts                      # citty group: json, notion
      json.ts                       # JSON database backup
      notion.ts                     # Notion database sync
    fetch/
      index.ts                      # citty command: fetch <ids>
    health/
      index.ts                      # prerequisites and system health
    ops/
      index.ts                      # citty group: prune, verify
      prune.ts                      # remove listings by criteria
      verify.ts                     # check if listings still active
    score/
      compute.ts                    # re-score all listings
      explain.ts                    # score breakdown for one listing
      index.ts                      # citty group: compute, explain
    search/
      diff.ts                       # compare portal IDs against database
      discover.ts                   # find portal IDs on Rightmove
      index.ts                      # citty group: discover, diff, resolve
      resolve.ts                    # resolve location name to REGION ID
    tools/
      index.ts                      # capability discovery from registry
    view/
      detail.ts                     # full listing data
      index.ts                      # citty group: detail, list
      list.ts                       # ranked listings table
  output/
    index.ts                        # human terminal formatters (preserved)
```

### Files Removed (Legacy Cleanup)

```
packages/cli/src/commands/fetch.ts          # replaced by fetch/index.ts
packages/cli/src/commands/assess.ts         # replaced by assess/*.ts
packages/cli/src/commands/view/index.ts     # replaced by new view/*.ts
packages/cli/src/commands/output/index.ts   # replaced by export/*.ts
packages/cli/src/commands/ops/index.ts      # replaced by new ops/*.ts
packages/cli/src/commands/ops/enrich.ts     # absorbed into fetch
packages/cli/src/commands/ops/prune.ts      # replaced by new ops/prune.ts
packages/cli/src/commands/ops/verify.ts     # replaced by new ops/verify.ts
packages/cli/src/commands/shared.ts         # replaced by core/paths.ts
packages/cli/src/commands/shared-read.ts    # replaced by core/paths.ts
packages/cli/src/commands/shared-write.ts   # logic distributed to fetch/index.ts
packages/cli/src/commands/help.ts           # replaced by tools/index.ts
```

---

## Cross-Platform Path Resolution

### Module: `packages/core/src/paths.ts` (shared between CLI and core)

Single `resolvePaths()` function, cached after first call. CLI primes it with overrides at startup; core enrichment code calls it (cache hit) to find source DBs.

**Precedence** (highest to lowest):

| Priority | Source             | Example                                                  |
| -------- | ------------------ | -------------------------------------------------------- |
| 1        | CLI flags          | `--data-dir /opt/let/data`                               |
| 2        | Category env vars  | `LET_DATA_DIR=/opt/let/data`                             |
| 3        | `LET_HOME` env var | `LET_HOME=/opt/let` (base for data/cache/config/sources) |
| 4        | Dev mode detection | Monorepo root found via `package.json` marker            |
| 5        | OS defaults        | XDG on Linux, `~/Library/Application Support` on macOS   |

**Dev mode detection**: Walk up from `process.cwd()` (max 5 levels) looking for `package.json` with `name: "let"` and `workspaces` field. If found, that directory is the monorepo root.

**OS defaults:**

| Category | Linux                                                        | macOS                                       |
| -------- | ------------------------------------------------------------ | ------------------------------------------- |
| Config   | `$XDG_CONFIG_HOME/let` or `~/.config/let`                    | `~/Library/Application Support/let`         |
| Data     | `$XDG_DATA_HOME/let` or `~/.local/share/let`                 | `~/Library/Application Support/let`         |
| Cache    | `$XDG_CACHE_HOME/let` or `~/.cache/let`                      | `~/Library/Caches/let`                      |
| Sources  | `$XDG_DATA_HOME/let/sources` or `~/.local/share/let/sources` | `~/Library/Application Support/let/sources` |

**Dev mode paths** (monorepo detected):

| Category | Path (relative to repo root) |
| -------- | ---------------------------- |
| Config   | `data/`                      |
| Data     | `data/`                      |
| Cache    | `.cache/`                    |
| Sources  | `sources/db/`                |

**Interface:**

```typescript
interface PathOverrides {
	dataDir?: string;
	configDir?: string;
	cacheDir?: string;
	sourcesDir?: string;
}

interface ResolvedPaths {
	config: string; // directory containing config file
	data: string; // directory containing let.db
	cache: string; // directory containing {portalId}/ cache entries
	sources: string; // directory containing *.db source databases
	isDev: boolean; // true when running from monorepo checkout
}

// Derived paths (computed from resolved dirs, never hardcoded)
interface DerivedPaths {
	configFile: string; // join(config, isDev ? 'let.config.toml' : 'config.toml')
	templateFile: string; // join(config, 'let.config.template.toml') -- dev only
	envFile: string; // join(config, '.env')
	database: string; // join(data, 'let.db')
	backup: string; // join(data, 'let.db.bak')
	jsonExport: string; // join(data, 'let.db.json')
	sourceDb(name: string): string; // join(sources, `${name}.db`)
	cacheDir(id: string): string; // join(cache, id)
	cacheEntry(id: string): string; // join(cache, id, 'data.json')
}

// Cached singleton
let cached: { resolved: ResolvedPaths; derived: DerivedPaths } | null = null;

export function resolvePaths(overrides?: PathOverrides): {
	resolved: ResolvedPaths;
	derived: DerivedPaths;
};
export function paths(): { resolved: ResolvedPaths; derived: DerivedPaths }; // shorthand, throws if not yet resolved
```

**Migration**: Replace the three duplicated path resolution functions:

- `packages/cli/src/commands/shared-read.ts`: `resolveRootDir()`, `ROOT_DIR`, `CACHE_DIR`, `DATA_DIR`, `LISTINGS_DB_PATH`, `CONFIG_PATH`
- `packages/core/src/pipeline/enrich/area.ts`: `resolveSourcesDir()`
- `packages/core/src/pipeline/enrich/broadband.ts`: `resolveDatabasePath()`

All replaced by imports from `packages/core/src/paths.ts`. Enrichment functions call `paths()` to get the cached resolved paths (CLI has already primed it at startup).

**Key rule**: All path strings in health check output, error hints, and remediation commands must be derived from `paths()`, never hardcoded. E.g., `fix: [\`cp \${paths().derived.templateFile} \${paths().derived.configFile}\`]`.

---

## Output Contracts

### JSON Envelope

Every command with `--json` outputs **exactly one JSON line** to stdout. No other output on stdout in `--json` mode.

```typescript
// Success
{
    ok: true,
    data: T,
    meta: {
        tool: string,        // e.g. "search.discover"
        elapsed: number,     // milliseconds
        count?: number,      // items in data (when data is array)
        total?: number,      // total matching (before limit)
        hasMore?: boolean    // true if more results available
    }
}

// Error
{
    ok: false,
    error: {
        code: string,        // e.g. "NO_CONFIG", "RATE_LIMITED"
        message: string,     // human-readable description
        hint: string         // actionable, copy-ready, uses resolved paths
    },
    meta: {
        tool: string,
        elapsed: number
    }
}
```

### Partial Success Pattern

For multi-item operations (`fetch <ids>`, `ops verify`, `export notion`), return `ok: true` with both `fetched[]`/`succeeded[]` and `failed[]` arrays. Only return `ok: false` if the entire operation cannot start (no config, no DB, permissions error). This lets the agent process results incrementally.

### Error Codes (Fixed Set)

| Code               | Meaning                       | Hint Pattern                           |
| ------------------ | ----------------------------- | -------------------------------------- |
| `NO_CONFIG`        | Config file not found         | Copy template path (from `paths()`)    |
| `INVALID_CONFIG`   | Config fails Zod validation   | Field-specific errors                  |
| `NO_DATABASE`      | Listings DB not found         | Run `let fetch <ids> --json`           |
| `DB_ERROR`         | SQLite operation failed       | Recovery from `paths().derived.backup` |
| `SCHEMA_MISMATCH`  | DB schema incompatible        | Backup + recreate instructions         |
| `NO_SOURCES`       | Required source DB missing    | Build command with resolved path       |
| `NOT_FOUND`        | Listing ID not in database    | `let fetch <portalId> --json`          |
| `RATE_LIMITED`     | Rightmove rate limit hit      | Wait + retry with `--delay`            |
| `PARSE_ERROR`      | Failed to extract PAGE_MODEL  | Listing may be removed                 |
| `VALIDATION_ERROR` | Input fails schema validation | Zod field errors                       |
| `API_ERROR`        | External API failure          | Service-specific                       |

### Stream Separation (Strict)

- **stdout**: In `--json` mode, exactly one JSON object (the envelope). In text mode, human-readable output only. **No** `console.log()`, progress bars, or diagnostic messages on stdout.
- **stderr**: All logs via existing structured logger (`log.cli`, `log.fetch`, etc.). Progress indicators. Debug output.
- **Exit codes**: 0 = success (including partial success), 1 = runtime error, 2 = prerequisites blocked.

### Envelope Module: `packages/cli/src/envelope.ts`

```typescript
function ok<T>(
	tool: string,
	data: T,
	start: number,
	meta?: Partial<Meta>,
): never; // writes JSON, exits 0
function fail(
	tool: string,
	code: string,
	message: string,
	hint: string,
	start: number,
): never; // writes JSON, exits 1|2
function isJsonMode(): boolean; // checks --json in process.argv (fast, no citty dependency)
```

The `ok()` and `fail()` functions use `process.stdout.write()` (not `console.log()`, which adds a newline and could be intercepted). They call `process.exit()` after writing to guarantee exactly one JSON line.

---

## Tool Registry: Single Source of Truth

### Design: `defineToolCommand()` Wrapper

File: `packages/cli/src/tool.ts`

Instead of a separate `registry.ts` that must be kept in sync with citty command definitions, we use a wrapper around `defineCommand` that captures metadata at definition time:

```typescript
import { defineCommand, type CommandDef } from "citty";

// Global registry populated as side effect of defineToolCommand()
export const toolRegistry: ToolMeta[] = [];

interface ToolMeta {
	name: string; // "search.discover"
	command: string; // "let search discover"
	description: string;
	category: string; // "search"
	parameters: ParameterMeta[];
	outputFields: string[];
	idempotent: boolean;
	rateLimit: string | null;
	example: string;
}

interface ToolCommandDef extends CommandDef {
	toolMeta: Omit<ToolMeta, "parameters" | "description">;
	// parameters extracted from citty args, description from meta
}

export function defineToolCommand(def: ToolCommandDef) {
	// Extract parameter metadata from citty args definition
	const parameters = extractParametersFromArgs(def.args);

	// Register in global registry
	toolRegistry.push({
		...def.toolMeta,
		description: def.meta?.description ?? "",
		parameters,
	});

	// Return a standard citty command (strip toolMeta)
	return defineCommand(def);
}
```

This guarantees:

- **One definition** per command (citty args + routing + tool metadata in one place)
- **No drift** between what citty routes and what `tools` reports
- **Parameters extracted from citty args** (the actual routing definitions)
- **`tools` command** simply serializes `toolRegistry[]`

### Drift Prevention Test

A test asserts that every command registered in the citty root `subCommands` tree has a corresponding entry in `toolRegistry`. If a new command is added via plain `defineCommand` (forgetting `defineToolCommand`), the test fails.

```typescript
// packages/cli/tests/registry.test.ts
test("all commands are registered in tool registry", () => {
	const registeredNames = new Set(toolRegistry.map((t) => t.name));
	const expectedNames = extractCommandNamesFromCittyTree(rootCommand);
	expect(registeredNames).toEqual(expectedNames);
});
```

---

## MVP Tool Surface

19 commands across 10 prefixes. Alphabetically sorted.

```
ASSESS    Assessment workflow
  assess candidates               List unassessed listings ranked by score
  assess context <id>             Context bundle for AI assessment
  assess submit <id>              Write assessment back

CONFIG    Configuration
  config show [section]           Show parsed configuration
  config validate                 Validate config file

EXPORT    External output
  export json [--output path]     JSON database backup
  export notion [--top N]         Notion database sync

FETCH     Data acquisition
  fetch <ids>                     Scrape, enrich, score, persist listings

HEALTH    Prerequisites
  health                          Check prerequisites and system health

OPS       Maintenance
  ops prune                       Remove listings by criteria
  ops verify                      Check if listings still active

SCORE     Scoring
  score compute                   Re-score all listings
  score explain <id>              Score breakdown for one listing

SEARCH    Discovery
  search discover                 Find portal IDs on Rightmove
  search diff <ids>               Compare portal IDs against database
  search resolve <name>           Resolve location name to REGION ID

TOOLS     Meta
  tools [name]                    Capability discovery

VIEW      Display
  view detail <id>                Full listing data
  view list                       Ranked listings table
```

### Singleton Prefix Rationale

Three prefixes have a single command: `fetch`, `health`, `tools`.

- **`fetch`** is standalone because fetching and searching are semantically different actions. `search discover` finds IDs on Rightmove; `fetch` scrapes, enriches, and persists. Merging under `search` would be misleading.
- **`health`** is a meta command that checks the entire system. Grouping with `tools` under a `meta` prefix adds indirection for no clarity benefit.
- **`tools`** returns the tool catalog. It's the agent's first call and should be maximally discoverable as a top-level command.

All three are high-frequency, conceptually distinct, and benefit from being top-level.

### Command Specifications

#### `tools [name]`

Returns the full tool catalog or detail for a single tool. Generated from `toolRegistry[]` (single source of truth via `defineToolCommand()`).

**JSON data** (catalog):

```json
{
	"version": "0.0.1",
	"tools": [
		{
			"name": "search.discover",
			"command": "let search discover",
			"description": "Find portal IDs on Rightmove for all configured locations",
			"category": "search",
			"parameters": [
				{
					"name": "--region",
					"type": "string",
					"required": false,
					"description": "Filter to specific region"
				},
				{
					"name": "--limit",
					"type": "number",
					"required": false,
					"description": "Max results per location"
				}
			],
			"outputFields": ["locations", "summary"],
			"idempotent": true,
			"rateLimit": "rightmove",
			"example": "let search discover --json"
		}
	],
	"globalFlags": [
		{ "name": "--json", "description": "Output as JSON envelope" },
		{ "name": "--data-dir", "description": "Override data directory" },
		{ "name": "--cache-dir", "description": "Override cache directory" },
		{ "name": "--config-dir", "description": "Override config directory" },
		{ "name": "--sources-dir", "description": "Override sources directory" }
	]
}
```

Output is alphabetically sorted by `category`, then by `name` within category.

#### `health`

Checks all prerequisites. Agent's first call in every session. All paths and fix commands use resolved paths from `paths()`.

**JSON data**:

```json
{
	"status": "ready|degraded|blocked",
	"paths": {
		"config": "/Users/han/Git/let/data",
		"data": "/Users/han/Git/let/data",
		"cache": "/Users/han/Git/let/.cache",
		"sources": "/Users/han/Git/let/sources/db",
		"isDev": true
	},
	"checks": [
		{
			"id": "config",
			"label": "Configuration",
			"status": "ok|missing|invalid",
			"severity": "blocking|degraded|info",
			"detail": "/Users/han/Git/let/data/let.config.toml",
			"fix": [
				"cp /Users/han/Git/let/data/let.config.template.toml /Users/han/Git/let/data/let.config.toml"
			]
		},
		{
			"id": "source.postcodes",
			"label": "Source: postcodes",
			"status": "ok|missing|outdated",
			"severity": "blocking",
			"detail": "124.3MB, 45d old",
			"fix": ["bun run sources:postcodes"]
		},
		{
			"id": "database",
			"label": "Listings Database",
			"status": "ok|missing|schema_mismatch",
			"severity": "info|blocking",
			"detail": "/Users/han/Git/let/data/let.db (47 listings)",
			"fix": null
		}
	],
	"summary": { "ok": 14, "blocking": 0, "degraded": 2 }
}
```

**Checks performed** (in order):

1. Config file existence + Zod validation (blocking)
2. Listings database existence + schema compatibility (info if missing, blocking if schema mismatch)
3. Source: postcodes (blocking -- required for area lookups)
4. Sources: broadband, deprivation, census, population, income, flood, crime, uprn, naptan (degraded)
5. EPC API credentials in `.env` (degraded)
6. Notion API credentials in `.env` (degraded)
7. Mapbox token in `.env` (degraded)
8. Directory write permissions: data dir, cache dir (blocking)

Exit code 0 if ready/degraded, 2 if blocked.

#### `config show [section]`

**Syntax**: `let config show [search|fetch|scoring] --json`

Returns parsed, validated config (full or section). Includes resolved paths so agent knows where config lives.

**JSON data**:

```json
{
    "path": "/Users/han/Git/let/data/let.config.toml",
    "config": { "search": { ... }, "fetch": { ... }, "scoring": { ... } }
}
```

#### `config validate`

Returns `{ valid: boolean, path: string, errors: Array<{ path: string, message: string }> }`.

#### `search discover`

**Syntax**: `let search discover [--region name] [--limit N] --json`

Searches all configured locations (or one if `--region`) on Rightmove. Returns portal IDs only -- no fetching, no persistence.

**JSON data**:

```json
{
	"locations": [
		{
			"name": "Sheffield",
			"id": "REGION^1195",
			"total": 84,
			"portalIds": ["170112233", "170112234"]
		}
	],
	"summary": { "total": 96, "locations": 2 }
}
```

#### `search diff <ids>`

**Syntax**: `let search diff 170112233,170112234,170112235 --json`

Compares comma-separated portal IDs against existing database.

**JSON data**:

```json
{ "new": ["170112235"], "known": ["170112233", "170112234"], "totalInDb": 47 }
```

#### `search resolve <name>`

**Syntax**: `let search resolve "Manchester" --json`

Resolves location name to Rightmove REGION identifiers via TypeAhead API.

**JSON data**:

```json
{
	"query": "Manchester",
	"results": [
		{ "name": "Manchester, Greater Manchester", "id": "REGION^904" }
	]
}
```

#### `fetch <ids>`

**Syntax**: `let fetch 170112233,170112235 [--region name] [--delay N] [--force] [--skip-images] --json`

Fetch, parse, enrich, score, persist specific listings. Re-scores all listings after merge (percentile normalization requires full dataset).

**Partial success**: Returns `ok: true` even if some IDs fail. Agent reads `failed[]` to decide on retries.

**JSON data**:

```json
{
	"fetched": [
		{
			"portalId": "170112233",
			"id": "uuid-...",
			"address": "3 Oak Lane, S10",
			"price": 950,
			"score": 78,
			"outcome": "new"
		}
	],
	"failed": [
		{
			"portalId": "170112299",
			"error": "404 Not Found",
			"hint": "Listing may have been removed"
		}
	],
	"summary": { "fetched": 2, "failed": 1, "totalInDb": 49 }
}
```

#### `view list`

**Syntax**: `let view list [--top N] [--min-score N] [--sort score|price|bedrooms|date] [--asc] [--region name] [--type types] [--status active|inactive|all] [--unassessed] --json`

**JSON data**: Array of listing summaries (16-field projection):

```json
{
	"listings": [
		{
			"id": "uuid-...",
			"portalId": "170112233",
			"address": "3 Oak Lane, Sheffield S10 3DA",
			"region": "Sheffield",
			"price": 950,
			"bedrooms": 3,
			"propertyType": "Semi-Detached",
			"score": 78,
			"assessedScore": 83,
			"recommendation": "recommend",
			"epcRating": "C",
			"floorAreaSqm": 82,
			"nearestStation": { "name": "Dore & Totley", "miles": 1.2 },
			"gigabitPct": 94,
			"status": "active",
			"url": "https://www.rightmove.co.uk/properties/170112233"
		}
	],
	"total": 47,
	"filtered": 20
}
```

#### `view detail <id>`

**Syntax**: `let view detail <id> [--open] --json`

Returns the complete `Listing` object (100+ fields). Agent's deep-dive command.

#### `score compute`

**Syntax**: `let score compute [--dry-run] --json`

Re-scores ALL listings using current config.

**JSON data**:

```json
{
	"scored": 47,
	"range": { "min": 32, "max": 91, "avg": 67, "median": 69 },
	"topChanges": [
		{
			"id": "uuid-...",
			"portalId": "170112233",
			"old": 72,
			"new": 78,
			"delta": 6
		}
	],
	"saved": true
}
```

#### `score explain <id>`

**Syntax**: `let score explain <id> --json`

Returns full scoring breakdown. Pure read from stored scores.

**JSON data**:

```json
{
	"id": "uuid-...",
	"overall": 78,
	"assessedScore": 83,
	"confidence": 0.85,
	"composites": {
		"affordability": {
			"score": 72,
			"weight": 0.35,
			"factors": { "trueMonthlyCost": 1180, "pricePctile": 65 }
		},
		"location": {
			"score": 85,
			"weight": 0.35,
			"factors": { "stationMiles": 1.2, "regionPriority": 90 }
		},
		"liveability": {
			"score": 76,
			"weight": 0.3,
			"factors": { "gigabitPct": 94, "epcNumeric": 68 }
		}
	},
	"penalties": { "epc": 1.0, "garden": 1.0, "pets": 0.85, "combined": 0.85 }
}
```

#### `assess candidates`

**Syntax**: `let assess candidates [--top N] [--region name] [--min-score N] --json`

**JSON data**:

```json
{
	"candidates": [
		{
			"id": "uuid-...",
			"portalId": "170112233",
			"address": "3 Oak Lane",
			"score": 78,
			"region": "Sheffield",
			"url": "..."
		}
	],
	"total": 47,
	"assessed": 12,
	"remaining": 35
}
```

#### `assess context <id>`

**Syntax**: `let assess context <id> --json`

Returns everything an agent needs to assess a listing in one call.

**JSON data**:

```json
{
	"listing": {
		/* full Listing object */
	},
	"scoreBreakdown": {
		/* same as score explain output */
	},
	"assessmentSchema": {
		/* JSON Schema derived from Zod */
	},
	"media": {
		"images": [
			"/Users/han/Git/let/.cache/170112233/170112233-photo-abc123.webp"
		],
		"floorplan": "/Users/han/Git/let/.cache/170112233/170112233-floorplan-def456.webp",
		"satellite": "/Users/han/Git/let/.cache/170112233/170112233-satellite-ghi789.webp",
		"street": "/Users/han/Git/let/.cache/170112233/170112233-street-jkl012.webp"
	},
	"links": {
		"rightmove": "https://www.rightmove.co.uk/properties/170112233",
		"googleMaps": "https://maps.google.com/...",
		"streetView": "https://maps.google.com/..."
	},
	"description": "Spacious 3-bed semi...",
	"notes": ["South-facing garden", "Gas central heating", "Double glazing"]
}
```

Media paths are **absolute**, derived from `paths().resolved.cache`.

#### `assess submit <id>`

**Syntax**: `let assess submit <id> --data '{...}' --json`

Validates against `AssessmentSchema`, computes assessed score, persists.

**JSON data**:

```json
{
	"id": "uuid-...",
	"portalId": "170112233",
	"algoScore": 78,
	"scoreAdjustment": 5,
	"assessedScore": 83,
	"recommendation": "recommend",
	"saved": true
}
```

#### `ops verify`

**Syntax**: `let ops verify [--region name] [--limit N] [--delay N] [--dry-run] --json`

Partial success pattern: returns `ok: true` with per-listing results.

#### `ops prune`

**Syntax**: `let ops prune [--min-score N] [--bottom N] [--region name] [--inactive] [--dry-run] --json`

No interactive prompts. `--dry-run` for preview.

#### `export json` / `export notion`

Standard patterns. `export notion` uses partial success (`ok: true` with `failed[]`).

---

## Health Check System

### Source Database Registry

Static metadata embedded in health check module:

| Source      | Required | Severity | Build Command                 |
| ----------- | -------- | -------- | ----------------------------- |
| postcodes   | yes      | blocking | `bun run sources:postcodes`   |
| broadband   | no       | degraded | `bun run sources:broadband`   |
| deprivation | no       | degraded | `bun run sources:deprivation` |
| census      | no       | degraded | `bun run sources:census`      |
| population  | no       | degraded | `bun run sources:population`  |
| income      | no       | degraded | `bun run sources:income`      |
| flood       | no       | degraded | `bun run sources:flood`       |
| crime       | no       | degraded | `bun run sources:crime`       |
| naptan      | no       | degraded | `bun run sources:naptan`      |
| uprn        | no       | degraded | `bun run sources:uprn`        |

Source DB paths are resolved via `paths().derived.sourceDb(name)`, not hardcoded.

### Bootstrap Flow

Agent runs `let health --json`. Based on response, it reads `fix[]` arrays and executes them. No special bootstrap tool -- the agent composes from health output + shell.

---

## Staged Refactor Sequence

### Stage 0: Foundation (Non-Breaking, Additive Only)

Create the infrastructure that all new commands will use. No existing behavior changes. Old commands continue to work.

**Create:**
| File | Purpose |
|------|---------|
| `packages/core/src/paths.ts` | Cross-platform path resolution (`resolvePaths`, `paths`, `DerivedPaths`) |
| `packages/cli/src/envelope.ts` | JSON envelope helpers (`ok`, `fail`, `isJsonMode`) |
| `packages/cli/src/tool.ts` | `defineToolCommand()` wrapper + `toolRegistry[]` |

**Modify:**
| File | Change |
|------|--------|
| `packages/core/src/index.ts` | Add `paths.ts` exports |
| `packages/core/src/pipeline/enrich/area.ts` | Import `paths()` from `core/paths.ts`, replace internal `resolveSourcesDir()` |
| `packages/core/src/pipeline/enrich/broadband.ts` | Import `paths()` from `core/paths.ts`, replace internal `resolveDatabasePath()` |

**Tests created:**
| File | Tests |
|------|-------|
| `packages/core/tests/paths.test.ts` | Dev mode detection, XDG resolution, macOS paths, `LET_HOME` override, CLI flag override, cache invalidation |
| `packages/cli/tests/envelope.test.ts` | `ok()` produces valid JSON, `fail()` produces valid JSON, no extra stdout |

**Verification**: `bun run util:check` passes. `bun test` passes. All existing commands still work (backward compat in enrichment via `paths()` cache).

### Stage 1: Meta Commands

**Create:**
| File | Command |
|------|---------|
| `packages/cli/src/commands/tools/index.ts` | `tools [name]` -- reads `toolRegistry[]` |
| `packages/cli/src/commands/health/index.ts` | `health` -- prereqs + source DB checks |

**Modify:**
| File | Change |
|------|--------|
| `packages/cli/src/index.ts` | Register `tools` and `health` alongside existing commands |

**Verification**: `let tools --json | bun -e "JSON.parse(require('fs').readFileSync('/dev/stdin','utf8'))"` -- valid JSON. `let health --json` -- valid JSON with resolved paths. Old commands still work.

### Stage 2: Read-Only Commands

Build commands that read from the database without modification.

**Create:**
| File | Commands |
|------|----------|
| `packages/cli/src/commands/config/index.ts` | Group: show, validate |
| `packages/cli/src/commands/config/show.ts` | `config show [section]` |
| `packages/cli/src/commands/config/validate.ts` | `config validate` |
| `packages/cli/src/commands/view/list.ts` | `view list` (with --json) |
| `packages/cli/src/commands/view/detail.ts` | `view detail <id>` (with --json) |
| `packages/cli/src/commands/score/index.ts` | Group: compute, explain |
| `packages/cli/src/commands/score/explain.ts` | `score explain <id>` |
| `packages/cli/src/commands/assess/candidates.ts` | `assess candidates` |
| `packages/cli/src/commands/assess/context.ts` | `assess context <id>` |

**Modify:**
| File | Change |
|------|--------|
| `packages/cli/src/index.ts` | Register `config`, `score` groups; update `view` and `assess` group routing |
| `packages/cli/src/commands/view/index.ts` | Rewrite as citty group routing to list.ts + detail.ts |

**Verification**: Each new command works with `--json` (stdout is parseable JSON). Human-readable text output renders correctly. Old commands still work alongside new ones.

### Stage 3: Search + Fetch

Build data acquisition tools.

**Create:**
| File | Commands |
|------|----------|
| `packages/cli/src/commands/search/index.ts` | Group: discover, diff, resolve |
| `packages/cli/src/commands/search/discover.ts` | `search discover` |
| `packages/cli/src/commands/search/diff.ts` | `search diff <ids>` |
| `packages/cli/src/commands/search/resolve.ts` | `search resolve <name>` |
| `packages/cli/src/commands/fetch/index.ts` | `fetch <ids>` (new, atomic, partial success) |

**Key refactoring**: Extract from current `fetch.ts` and `shared-write.ts`:

- Search-only logic (results without fetching) -> `search/discover.ts`
- `processListing()` pipeline -> `fetch/index.ts`
- Location resolver -> `search/resolve.ts`
- DB diff logic (compare portal IDs vs existing) -> `search/diff.ts`

**Verification**: `let search discover --json` returns IDs. `let search diff 123,456 --json` classifies correctly. `let fetch 170448131 --json` fetches and persists with partial success on mixed IDs.

### Stage 4: Mutation Commands

Build remaining write operations.

**Create:**
| File | Commands |
|------|----------|
| `packages/cli/src/commands/score/compute.ts` | `score compute` |
| `packages/cli/src/commands/assess/submit.ts` | `assess submit <id>` |

**Modify:**
| File | Change |
|------|--------|
| `packages/cli/src/commands/ops/index.ts` | Rewrite as citty group: prune, verify |
| `packages/cli/src/commands/ops/prune.ts` | Add `--json`, `--inactive` flag, partial success |
| `packages/cli/src/commands/ops/verify.ts` | Add `--json`, partial success |

**Parity gate**: At this point, run the full agent workflow end-to-end using ONLY new commands:

```
tools --json -> health --json -> config show --json -> search discover --json ->
search diff <ids> --json -> fetch <ids> --json -> view list --json ->
assess context <id> --json -> assess submit <id> --data '...' --json -> view list --json
```

This must complete successfully before proceeding to cleanup.

### Stage 5: Export Commands

**Create:**
| File | Commands |
|------|----------|
| `packages/cli/src/commands/export/index.ts` | Group: json, notion |
| `packages/cli/src/commands/export/json.ts` | `export json` |
| `packages/cli/src/commands/export/notion.ts` | `export notion` |

### Stage 6: Cleanup (Clean Cut)

Remove all legacy files. This stage happens immediately after the parity gate passes.

**Rewrite:**
| File | Change |
|------|--------|
| `packages/cli/src/index.ts` | 10 subcommand groups only (no legacy commands) |

**Delete:** All files in "Files Removed" section.

**Verification**: `let tools --json` shows only new commands. No old command names work. `bun run util:check` clean. `bun run build:cli` produces working binary. Binary smoke test: `bin/let health --json` works.

### Stage 7: Skill + Documentation

**Create:**
| File | Purpose |
|------|---------|
| `.claude/commands/let.md` | Master agent skill prompt |

**Modify:**
| File | Change |
|------|--------|
| `CLAUDE.md` | Update repository structure, commands section, CLI section |
| `.claude/commands/assess.md` | Update to use new command syntax |
| `.claude/commands/summarize.md` | Update to use new command syntax |

---

## `/let` Skill Content

File: `.claude/commands/let.md`

### 1. Orient

```bash
let tools --json               # discover available commands
let health --json              # check prerequisites
let config show --json         # understand locations, scoring weights, filters
```

If health returns `blocked`: execute fix commands from `checks[].fix`, re-check. If `degraded`: proceed with partial enrichment.

### 2. Discover

```bash
let search discover --json                     # find portal IDs across all configured locations
let search diff <comma-separated-ids> --json   # classify new vs known
```

### 3. Acquire

```bash
let fetch <new-ids> --json                     # fetch in batches of 10-15
```

Check `failed[]` -- retry once for transient errors, skip permanent (404).

### 4. Triage

```bash
let view list --top 30 --json                  # ranked overview
```

Classify: score >= 80 (must assess), 65-79 (assess if time), < 65 (skip).

### 5. Assess (for each top candidate)

```bash
let assess context <id> --json                 # get everything needed
```

Then: Read images from `media.images` paths (Glob + Read), review satellite/street maps, check `scoreBreakdown`, web search neighborhood. Submit:

```bash
let assess submit <id> --data '{"maintenance":"good","lightAndSpace":"...","photoAnalysis":"...","recommendation":"recommend","familySuitability":"good","reasoning":"...","scoreAdjustment":5}' --json
```

### 6. Report

```bash
let view list --top 20 --json                  # final rankings
```

Present top 3-5 with: links, score breakdowns, assessment summaries, neighborhood context, tradeoffs, next steps.

### Error Recovery Table

| Error Code         | Agent Action                                                                                        |
| ------------------ | --------------------------------------------------------------------------------------------------- |
| `NO_CONFIG`        | Tell user to create config from template. Cannot proceed.                                           |
| `NO_SOURCES`       | Tell user: run build command from `fix[]`. Continue without -- scoring works with lower confidence. |
| `NO_DATABASE`      | Normal on first run. Proceed to fetch step.                                                         |
| `SCHEMA_MISMATCH`  | Tell user: backup exists at `paths.backup`. May need to recreate DB.                                |
| `RATE_LIMITED`     | Wait 60s, retry with `--delay 5000`.                                                                |
| `NOT_FOUND`        | Listing removed. Skip and continue.                                                                 |
| `VALIDATION_ERROR` | Fix assessment JSON (check enums, string lengths, scoreAdjustment -30 to +30).                      |
| `API_ERROR`        | Log, skip affected enrichment, continue with available data.                                        |

### Score Interpretation

| Range  | Meaning                                        |
| ------ | ---------------------------------------------- |
| 85-100 | Exceptional -- strong across all dimensions    |
| 70-84  | Good -- strong in most areas, minor weaknesses |
| 55-69  | Average -- mixed, moderate penalties           |
| 40-54  | Below average -- significant weaknesses        |
| < 40   | Poor -- major penalties                        |

Scores are percentile-relative. The agent adds value by detecting what photos reveal, researching neighborhoods, identifying tradeoffs the algorithm cannot weigh.

---

## Existing Core Utilities to Reuse (Unchanged)

| Function                   | Location                         | Used By                                   |
| -------------------------- | -------------------------------- | ----------------------------------------- |
| `searchListingsApi()`      | `core/pipeline/fetch/api.ts`     | `search discover`                         |
| `lookupLocation()`         | `core/pipeline/fetch/api.ts`     | `search resolve`                          |
| `scrapeSearchResults()`    | `core/pipeline/parse/extract.ts` | `search discover` (HTML fallback)         |
| `scrapeListing()`          | `core/pipeline/parse/extract.ts` | `fetch`                                   |
| `enrichListing()`          | `core/pipeline/enrich/index.ts`  | `fetch`                                   |
| `scoreListings()`          | `core/pipeline/score/index.ts`   | `fetch`, `score compute`                  |
| `calculateAssessedScore()` | `core/pipeline/assess/index.ts`  | `assess submit`                           |
| `queryListings()`          | `core/pipeline/view/index.ts`    | `view list`                               |
| `filterListings()`         | `core/pipeline/view/index.ts`    | `view list`, `assess candidates`          |
| `findListingById()`        | `core/pipeline/view/index.ts`    | `view detail`, `score explain`            |
| `computeStats()`           | `core/pipeline/view/index.ts`    | `health`                                  |
| `loadConfig()`             | `core/config/loader.ts`          | `config show`, `search discover`, `fetch` |
| `loadListingsFile()`       | `core/db/index.ts`               | All read commands                         |
| `saveListingsFile()`       | `core/db/index.ts`               | All write commands                        |
| `createNotionPage()`       | `core/pipeline/output/notion.ts` | `export notion`                           |
| Terminal formatters        | `cli/output/index.ts`            | All text-mode output                      |

---

## Testing and CI

### Contract Tests (First-Class)

**Stdout purity test**: For every command, a test runs the command with `--json`, captures stdout, and asserts:

1. Stdout parses as valid JSON
2. JSON has `ok` field (boolean)
3. If `ok: true`: has `data` and `meta` fields
4. If `ok: false`: has `error` with `code`, `message`, `hint` fields
5. No extra bytes before or after the JSON object

```typescript
// packages/cli/tests/contract.test.ts
for (const tool of toolRegistry) {
	test(`${tool.name}: stdout is valid JSON envelope`, async () => {
		const result = Bun.spawnSync([
			"bun",
			"run",
			"let",
			...tool.command.split(" ").slice(1),
			"--json",
		]);
		const stdout = result.stdout.toString();
		const parsed = JSON.parse(stdout); // must not throw
		expect(parsed).toHaveProperty("ok");
		expect(parsed).toHaveProperty("meta.tool", tool.name);
	});
}
```

### Path Resolution Tests

```typescript
// packages/core/tests/paths.test.ts
test('dev mode: detects monorepo root', () => { ... });
test('installed mode: uses XDG on Linux', () => { ... });
test('installed mode: uses Library on macOS', () => { ... });
test('LET_HOME overrides OS defaults', () => { ... });
test('category env vars override LET_HOME', () => { ... });
test('CLI flags override everything', () => { ... });
test('derived paths use resolved dirs', () => { ... });
```

### Registry Drift Test

```typescript
// packages/cli/tests/registry.test.ts
test("all citty commands have toolRegistry entries", () => {
	const registered = new Set(toolRegistry.map((t) => t.name));
	const cittyNames = extractFromCittyTree(rootCommand);
	expect(registered).toEqual(cittyNames);
});
```

### Quality Gates

1. `bun run util:check` -- format + lint + types + tests
2. `bun test` -- all tests pass (core unchanged + new CLI tests + contract tests)
3. `bun run build:cli` -- compiled binary produces valid executable
4. `bin/let health --json` -- smoke test compiled binary
5. `bin/let tools --json` -- tool discovery works from binary

### Per-Stage Smoke Tests

After each stage:

```bash
bun run let tools --json 2>/dev/null | bun -e "console.log(JSON.parse(require('fs').readFileSync('/dev/stdin','utf8')).ok)"
# Expected: true
```

---

## Acceptance Criteria

- [ ] Builds and runs on macOS and Linux
- [ ] Compiled binary uses OS-appropriate paths; dev checkout uses repo-local paths
- [ ] `let health --json` returns machine-readable checks with actionable fix commands using resolved paths
- [ ] `let tools --json` returns full catalog generated from `defineToolCommand()` definitions (single source of truth)
- [ ] Registry drift test passes (all citty commands have toolRegistry entries)
- [ ] End-to-end agent loop using only new toolbelt: `tools` -> `health` -> `config show` -> `search discover` -> `search diff` -> `fetch` -> `view list` -> `assess context` -> `assess submit` -> `view list`
- [ ] Every command supports `--json` with stable envelope shape
- [ ] Stdout purity: in `--json` mode, stdout is exactly one JSON object (contract tests pass)
- [ ] Exit codes: 0 = success/partial success, 1 = runtime error, 2 = prerequisites blocked
- [ ] Error responses include `code`, `message`, `hint` (hint uses resolved paths, not hardcoded)
- [ ] Multi-ID operations use partial success pattern (`ok: true` with `failed[]`)
- [ ] CLI surface is flat-but-prefixed, alphabetically sorted (10 prefixes, 19 commands)
- [ ] No legacy commands remain after cleanup
- [ ] Core domain internals unchanged (all existing core tests pass)
- [ ] DB ownership clear: `@let/core/db` owns DDL and all read/write; no raw SQL in CLI
- [ ] `/let` skill file created with full workflow, error recovery table, and score interpretation
- [ ] `bun run util:check` passes clean
- [ ] `bun run build:cli` produces working binary
