> `let` is a Rust workspace for an agent-native UK rental search toolbelt. It discovers Rightmove listings, enriches them from local source databases plus EPC/Mapbox/Notion integrations, scores them against configurable preferences, and serves the same working set through a JSON-first CLI and a Ratatui browser.

## 1. Documentation

- Primary runtime contracts: [`crates/let-cli/src/main.rs`](crates/let-cli/src/main.rs), [`crates/let-cli/src/registry.rs`](crates/let-cli/src/registry.rs), [`crates/let-cli/src/envelope.rs`](crates/let-cli/src/envelope.rs), [`crates/let-sdk/src/config.rs`](crates/let-sdk/src/config.rs), [`crates/let-sdk/src/paths.rs`](crates/let-sdk/src/paths.rs), [`crates/let-sdk/src/db/schema.sql`](crates/let-sdk/src/db/schema.sql)
- Source-build and enrichment truth: [`crates/let-sdk/src/sources/mod.rs`](crates/let-sdk/src/sources/mod.rs), [`crates/let-sdk/src/sources/common.rs`](crates/let-sdk/src/sources/common.rs), [`crates/let-sdk/src/pipeline/enrich.rs`](crates/let-sdk/src/pipeline/enrich.rs), [`crates/let-sdk/src/pipeline/epc.rs`](crates/let-sdk/src/pipeline/epc.rs)
- Runtime entrypoints: [`crates/let-cli/src/commands/start.rs`](crates/let-cli/src/commands/start.rs), [`crates/let-tui/src/app.rs`](crates/let-tui/src/app.rs), [`package.json`](package.json)
- Agent workflow docs live in [`.claude/skills/let/SKILL.md`](.claude/skills/let/SKILL.md); bootstrap templates live in [`.claude/skills/let/templates/`](.claude/skills/let/templates/)
- Prefer `let tools`, `let health`, and `let config show` over prose when command shape or runtime expectations are unclear
- [`.github/workflows/`](.github/workflows/) is currently empty, so local commands and hooks are the effective source of validation policy
- Treat [`.env.example`](.env.example) and [`crates/let-sdk/src/config.rs`](crates/let-sdk/src/config.rs) as more current than [`.claude/skills/let/templates/env.example`](.claude/skills/let/templates/env.example) or [`.claude/skills/let/templates/let.config.toml`](.claude/skills/let/templates/let.config.toml) when defaults drift

## 2. Repository Structure

```text
.
├── crates/
│   ├── let-cli/              clap command surface, tool registry, envelopes, clipboard
│   ├── let-sdk/              config, paths, DB, fetch/enrich/score pipelines, source builders
│   └── let-tui/              Ratatui browser over the shared DB and cache
├── .claude/skills/let/       agent workflow and bootstrap templates
├── package.json              Bun wrappers for hooks, builds, and local checks
├── AGENTS.md                 canonical repo instructions
├── CLAUDE.md -> AGENTS.md
└── README.md -> AGENTS.md
```

- Start in [`crates/let-cli/src/commands/`](crates/let-cli/src/commands/) for CLI surface changes, [`crates/let-sdk/src/pipeline/`](crates/let-sdk/src/pipeline/) for fetch/enrich/score behavior, and [`crates/let-sdk/src/db/`](crates/let-sdk/src/db/) for persistence changes
- [`crates/let-cli/src/registry.rs`](crates/let-cli/src/registry.rs) is the source of truth for the agent-facing tool catalog and global flags
- Treat [`target/`](target/), [`node_modules/`](node_modules/), `$LET_HOME/data`, `$LET_HOME/cache`, and `$LET_HOME/sources` as generated or runtime-owned state

## 3. Stack

| Layer | Choice | Notes |
| --- | --- | --- |
| Runtime | Rust 2024 workspace | three crates: SDK, CLI, TUI; `unsafe` forbidden |
| CLI | `clap` + JSON envelopes | default stdout is machine-facing; `--text` switches to human output |
| TUI | `ratatui` + `crossterm` | launched by the CLI and pointed at the same runtime dirs |
| Storage | SQLite via `rusqlite` | one listings DB plus one DB per enrichment source |
| HTTP / Parsing | `reqwest`, `serde_json`, `csv`, `zip`, `calamine`, `image` | Rightmove fetch, EPC/Mapbox/Notion, source ingests, media normalization |
| JS Tooling | Bun + Husky + lint-staged | wrappers, hooks, and release/install scripts only; product code is Rust |

## 4. Commands

- `bun install` installs JS tooling and activates the Husky hooks in [`.husky/`](.husky/)
- `cargo run -q -p let-cli -- tools` prints the current tool catalog and global flags from [`crates/let-cli/src/registry.rs`](crates/let-cli/src/registry.rs)
- `cargo run -q -p let-cli -- health` checks config, DB/schema health, source DB presence, credentials, and writable runtime dirs
- `cargo run -q -p let-cli -- search discover` and `cargo run -q -p let-cli -- search diff <ids>` are the discovery loop; discovery is API-first with HTML fallback unless `search.useApi = false`
- `cargo run -q -p let-cli -- fetch <ids>` fetches Rightmove pages, enriches, scores, optionally normalizes media, and upserts the listings DB
- `cargo run -q -p let-cli -- assess candidates|context|submit ...` is the structured assessment loop over stored listings
- `cargo run -q -p let-cli -- build sources list|all|<name>` builds enrichment DBs under `$LET_HOME/sources`; progress logs are written to stderr, not stdout
- `bun run build` performs a release build and installs `let` plus `let-tui` into `${LET_HOME:-${TOOLS_HOME:-$HOME/.tools}/let}`
- `bun run util:check` runs the local completion gate: fmt, clippy, `cargo check`, tests, and the release build/install wrapper

## 5. Architecture

- [`crates/let-cli/src/main.rs`](crates/let-cli/src/main.rs) parses flags, resolves path overrides, dispatches commands, and emits envelopes or text
- [`crates/let-cli/src/commands/`](crates/let-cli/src/commands/) should stay thin; persistence, parsing, enrichment, and scoring belong in `let-sdk`
- [`crates/let-sdk/src/pipeline/fetch/rightmove.rs`](crates/let-sdk/src/pipeline/fetch/rightmove.rs) turns Rightmove HTML `PAGE_MODEL` data into listings and classifies active vs let-agreed vs removed pages
- [`crates/let-sdk/src/pipeline/enrich.rs`](crates/let-sdk/src/pipeline/enrich.rs) joins local postcode, IMD, census, population, income, flood, crime, NaPTAN, and UPRN data; missing source DBs degrade the report instead of aborting most workflows
- [`crates/let-sdk/src/pipeline/score.rs`](crates/let-sdk/src/pipeline/score.rs) computes percentile-relative scores across the current DB, persists factor context, and derives `assessedScore` from saved assessments
- [`crates/let-cli/src/commands/start.rs`](crates/let-cli/src/commands/start.rs) launches `let-tui` as a sibling binary or via `LET_TUI_BIN` and forwards runtime dirs through `LET_*_DIR` env vars
- Default stdout is exactly one JSON envelope per command. Human text, progress, warnings, and confirmation prompts go to stderr or require `--text`. There is no supported `--json` flag.

## 6. Runtime and State

- Path precedence is CLI flags -> `LET_*_DIR` -> `LET_HOME` -> `TOOLS_HOME/let` -> `~/.tools/let`; see [`crates/let-sdk/src/paths.rs`](crates/let-sdk/src/paths.rs)
- Config and secrets live at `$LET_HOME/data/let.config.toml` and `$LET_HOME/data/.env`; `let health` also assumes `$LET_HOME/data` is writable
- The listings DB lives at `$LET_HOME/data/let.db`; backups default to `$LET_HOME/data/let.db.bak`; JSON export defaults to `$LET_HOME/data/let.db.json`
- Installed binaries normally live alongside those dirs at `$LET_HOME/let` and `$LET_HOME/let-tui`
- Media cache entries live under `$LET_HOME/cache/<rightmove-id-or-uuid>/`; normalized filenames include an asset kind, short hashes, and a `-v1.jpg` suffix from [`crates/let-sdk/src/pipeline/fetch/cache.rs`](crates/let-sdk/src/pipeline/fetch/cache.rs)
- Source DBs live under `$LET_HOME/sources/{broadband,postcodes,deprivation,census,population,income,flood,naptan,uprn,crime}.db`
- Source builds accept per-input path or URL override env vars plus optional `*_SHA256` integrity guards, and each built DB gets `source_runs` / `source_inputs` metadata written by [`crates/let-sdk/src/sources/common.rs`](crates/let-sdk/src/sources/common.rs)
- Listings DB schema version is `2`; scored rows also require `score_contexts`. Missing score context rows or schema-version mismatches are treated as `SCHEMA_MISMATCH` and the supported repair path is DB recreation, not hand migration.
- Environment variables that materially affect behavior: `EPC_API_BEARER_TOKEN` preferred, legacy `EPC_API_EMAIL` + `EPC_API_KEY`, `MAPBOX_ACCESS_TOKEN`, `NOTION_API_KEY`, `NOTION_DATABASE_ID`, `LET_TUI_BIN`, `LET_CLIPBOARD_BIN`, `LET_SKIP_DB_BACKUP`, `LET_DB_BACKUP_MIN_SECONDS`

## 7. Conventions

- Use Rightmove portal IDs for external-facing command workflows and cache directories; internal listing rows also have UUIDs, and commands like `view detail` / `assess submit` accept either
- Keep command payloads camelCase and envelope-safe; when adding or renaming fields, update [`crates/let-cli/src/registry.rs`](crates/let-cli/src/registry.rs) and any text renderers or clipboard payloads that mirror them
- `search.useApi` controls discovery transport only. `fetch.useApi` is intentionally rejected by [`crates/let-sdk/src/config.rs`](crates/let-sdk/src/config.rs).
- Batch `fetch` runs apply `fetch.minScore` before the heavy media stage; single-ID fetches skip that threshold unless `--min-score` is passed explicitly
- `fetch --override-postcode` and `--override-address` are single-ID correction tools; they update address-derived URLs and can trigger postcode or Mapbox-based coordinate re-resolution before enrichment and scoring
- `ops patch` re-enriches from source DBs by default, then reapplies the explicit patch so manual values win in that invocation; use `--skip-re-enrich` when you need a pure manual override
- `view ... --copy` copies pretty JSON in default mode and rendered text in `--text`; on non-macOS platforms you must provide `LET_CLIPBOARD_BIN`

## 8. Constraints

- Never hand-edit [`crates/let-sdk/src/db/schema.sql`](crates/let-sdk/src/db/schema.sql)-backed runtime DB files, source DBs, or cache assets; use CLI commands or source builders instead
- High-risk files are [`crates/let-sdk/src/db/schema.sql`](crates/let-sdk/src/db/schema.sql), [`crates/let-sdk/src/db/repository.rs`](crates/let-sdk/src/db/repository.rs), [`crates/let-sdk/src/pipeline/fetch/rightmove.rs`](crates/let-sdk/src/pipeline/fetch/rightmove.rs), [`crates/let-sdk/src/pipeline/epc.rs`](crates/let-sdk/src/pipeline/epc.rs), [`crates/let-sdk/src/sources/`](crates/let-sdk/src/sources/), and [`crates/let-cli/src/registry.rs`](crates/let-cli/src/registry.rs)
- DB writes create `.bak` snapshots unless `LET_SKIP_DB_BACKUP=1` is set; do not disable backups in normal development or runtime repair work
- `ops prune` is destructive and interactive in text mode; non-interactive automation must use `--dry-run` first and `--force` only when the selection is already understood
- `ops verify` mutates stored listing status unless `--dry-run` is set
- Rightmove, EPC, Mapbox, Notion, and public source datasets are unstable dependencies. Expect API fallback, removed listings, header/schema drift, and partial enrichment.
- Do not commit `$LET_HOME/data/.env`, exported listing snapshots, cached media, test-generated DBs, or personal search context

## 9. Validation

- Required local gate: `bun run util:check`
- CLI surface, envelope, clipboard, `--text`, or tool-registry changes: `cargo test -p let-cli --test cli_integration`
- DB schema or repository changes: `cargo test -p let-sdk --test db_tests`
- Discovery, fetch, enrichment, source-build, or scoring changes: run the relevant crate tests plus at least one targeted manual smoke flow
- Manual smoke for runtime-path or workflow changes: `cargo run -q -p let-cli -- tools`, `health`, `config show`, `search discover`, `fetch <id> --skip-images`, `view list`, `score explain <id>`, `export json`, `build sources list`
- If you touch [`crates/let-cli/src/registry.rs`](crates/let-cli/src/registry.rs), [`crates/let-cli/src/main.rs`](crates/let-cli/src/main.rs), or [`crates/let-cli/src/envelope.rs`](crates/let-cli/src/envelope.rs), verify default JSON mode still emits one stdout line and `--text` still produces readable output
