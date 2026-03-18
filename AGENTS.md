## 1. Documentation

- Purpose: run `let` as an agent-native UK property search/scoring toolbelt with composable primitives.
- Source priority: executable CLI contracts -> Rust code -> docs.
- Runtime truth commands:
    - `:let tools`
    - `:let health`
    - `:let config show`
- JSON contract: all non-interactive commands emit exactly one envelope object to stdout by default; logs go to stderr only.
- Skill entrypoint: `.claude/skills/let/SKILL.md`.
- External APIs:
    - EPC: `https://epc.opendatacommunities.org/docs/api`
    - Postcodes: `https://postcodes.io/docs`
    - Notion: `https://developers.notion.com/llms.txt`
    - Mapbox: `https://docs.mapbox.com/llms.txt`

## 2. Repository Structure

```text
.
├── Cargo.toml
├── crates/
│   ├── let-sdk/
│   │   └── src/{config,context,db,errors,pipeline,paths,schema,sources,utils}
│   ├── let-cli/
│   │   └── src/{main,envelope,registry,commands/*}
│   └── let-tui/
│       └── src/{main,app,ui,theme}
├── .claude/skills/let/
└── $LET_HOME/{data,cache,sources}/
```

- CLI registry source of truth: `crates/let-cli/src/registry.rs`.
- CLI envelope source of truth: `crates/let-cli/src/envelope.rs`.
- Path precedence source of truth: `crates/let-sdk/src/paths.rs`.
- Listings schema source of truth: `crates/let-sdk/src/db/schema.sql`.
- Source-build subsystem: `crates/let-sdk/src/sources/*`.

## 3. Stack

| Layer    | Choice                      | Notes                           |
| -------- | --------------------------- | ------------------------------- |
| Runtime  | Rust                        | Native binaries for SDK/CLI/TUI |
| Language | Rust 2024                   | Strict lints, no unsafe         |
| CLI      | clap                        | Typed command tree              |
| TUI      | ratatui + crossterm         | Dense cyan-themed terminal UI   |
| Storage  | SQLite (rusqlite)           | Listings DB + source DBs        |
| Parsing  | csv + calamine + zip        | Source ingest and transforms    |
| HTTP     | reqwest (rustls)            | Scraping + source downloads     |
| Quality  | cargo fmt/clippy/test/build | Required completion gates       |

- Optional environment keys: `EPC_API_BEARER_TOKEN`, legacy transition fallback `EPC_API_EMAIL` + `EPC_API_KEY`, `NOTION_API_KEY`, `NOTION_DATABASE_ID`, `MAPBOX_ACCESS_TOKEN`.
- Optional source integrity keys: `<SOURCE_INPUT>_SHA256` (for example `POSTCODES_ZIP_SHA256`) to enforce SHA-256 verification for local or downloaded source files.
- Storage root: `$LET_HOME` (default `~/.tools/let`).

## 4. Commands

- Bootstrap: `bun install` (commit hooks) and `cargo build --workspace`.
- Build binaries: `bun run build` or `cargo build --workspace --release`.
- Agent entrypoint: `:let <command>`.
- Dev entrypoint: `cargo run -q -p let-cli -- <command>`.
- Source builds:
    - `bun run build:sources`
    - `:let build sources list`
    - `:let build sources <name>`
    - `:let build sources all --jobs 3`
    - Optional: set matching checksum env keys before build to enforce source integrity verification.

- Infrastructure:
    - `let tools`
    - `let health`
- Core primitives:
    - `config show|validate`
    - `search resolve|discover|diff`
    - `fetch`
    - `view list|detail`
    - `score compute|explain`
    - `assess candidates|context|submit`
    - `export json|notion`
    - `ops patch|verify|prune`
    - `start` (launch TUI)

## 5. Architecture

- System shape: Rust workspace split into SDK (`let-sdk`), CLI (`let-cli`), and TUI (`let-tui`).
- Pipeline contract: Fetch -> Parse -> Enrich -> Score -> Assess -> View -> Output.
- Command design rule: keep commands primitive; orchestration belongs in the agent.
- Envelope contract:
    - Success: `{ ok: true, data, meta: { tool, elapsed, count?, total?, hasMore? } }`
    - Error: `{ ok: false, error: { code, message, hint }, meta: { tool, elapsed } }`
    - Exit: `0` success, `1` runtime failure, `2` blocked prerequisite.
- Path precedence:
    1. CLI flags (`--data-dir`, `--config-dir`, `--cache-dir`, `--sources-dir`)
    2. Env (`LET_DATA_DIR`, `LET_CONFIG_DIR`, `LET_CACHE_DIR`, `LET_SOURCES_DIR`)
    3. `LET_HOME` / `TOOLS_HOME` / default `~/.tools/let`
- Storage:
    - Listings DB: `$LET_HOME/data/let.db`
    - Backup DB: `$LET_HOME/data/let.db.bak`
    - JSON export: `$LET_HOME/data/let.db.json` or `--output`
    - Cache: `$LET_HOME/cache/{portalId}/...`
    - Source DBs: `$LET_HOME/sources/{postcodes,broadband,deprivation,census,population,income,flood,crime,naptan,uprn}.db`
- Safety policy: workspace and crate roots forbid unsafe code.

## 6. Quality

- Completion gates:
    - `cargo fmt --all --check`
    - `cargo clippy --workspace --all-targets --all-features -- -D warnings`
    - `cargo test --workspace`
    - `cargo build --workspace --release`

- Standard script wrapper: `bun run util:check`.
- Manual smoke flow:

```bash
bun run build
:let tools
:let health
:let config show
:let search discover
:let search diff <id1>,<id2>
:let fetch <id> --skip-images
:let view list
:let score explain <id>
:let export json
:let build sources naptan
```

- Risks to surface in outputs:
    - upstream source URL expiry or schema drift
    - Rightmove throttling / partial fetches
    - SQLite schema version mismatch requiring DB recreation
