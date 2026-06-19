> `let` is a Rust workspace for an agent-native UK rental intelligence toolbelt. It discovers Rightmove listing ids, gathers evidence bundles from Rightmove plus local source databases, Mapbox, and EPC data, verifies checkable claims, and persists AI-authored assessments through a JSON-first CLI.

## 1. Documentation

- Primary runtime contracts: [`crates/let-cli/src/main.rs`](crates/let-cli/src/main.rs), [`crates/let-cli/src/registry.rs`](crates/let-cli/src/registry.rs), [`crates/let-cli/src/envelope.rs`](crates/let-cli/src/envelope.rs), [`crates/let-sdk/src/intelligence/types.rs`](crates/let-sdk/src/intelligence/types.rs), [`crates/let-sdk/src/intelligence/repository.rs`](crates/let-sdk/src/intelligence/repository.rs), [`crates/let-sdk/src/intelligence/service.rs`](crates/let-sdk/src/intelligence/service.rs), [`crates/let-sdk/src/config.rs`](crates/let-sdk/src/config.rs), [`crates/let-sdk/src/paths.rs`](crates/let-sdk/src/paths.rs)
- Source-build and enrichment truth: [`crates/let-sdk/src/sources/mod.rs`](crates/let-sdk/src/sources/mod.rs), [`crates/let-sdk/src/sources/common.rs`](crates/let-sdk/src/sources/common.rs), [`crates/let-sdk/src/pipeline/enrich.rs`](crates/let-sdk/src/pipeline/enrich.rs), [`crates/let-sdk/src/pipeline/epc.rs`](crates/let-sdk/src/pipeline/epc.rs), [`crates/let-sdk/src/pipeline/fetch/rightmove.rs`](crates/let-sdk/src/pipeline/fetch/rightmove.rs)
- Runtime entrypoints: [`crates/let-cli/src/main.rs`](crates/let-cli/src/main.rs), [`crates/let-tui/src/app.rs`](crates/let-tui/src/app.rs), [`package.json`](package.json)
- Agent workflow docs live in [`.claude/skills/let/SKILL.md`](.claude/skills/let/SKILL.md); bootstrap templates live in [`.claude/skills/let/templates/`](.claude/skills/let/templates/)
- Prefer `let tools`, `let health`, and `let config show` over prose when command shape or runtime expectations are unclear
- [`.github/workflows/`](.github/workflows/) is currently empty, so local commands and hooks are the effective source of validation policy
- Treat [`.env.example`](.env.example) and [`crates/let-sdk/src/config.rs`](crates/let-sdk/src/config.rs) as more current than [`.claude/skills/let/templates/env.example`](.claude/skills/let/templates/env.example) or [`.claude/skills/let/templates/let.config.toml`](.claude/skills/let/templates/let.config.toml) when defaults drift

## 2. Repository Structure

```text
.
├── crates/
│   ├── let-cli/              clap command surface, tool registry, envelopes, command wrappers
│   ├── let-sdk/              config, paths, intelligence DB, Rightmove/EPC/Mapbox/source pipelines
│   └── let-tui/              Ratatui browser over local runtime data and cache
├── .claude/skills/let/       agent workflow and bootstrap templates
├── package.json              Bun wrappers for hooks, builds, and local checks
├── AGENTS.md                 canonical repo instructions
├── CLAUDE.md -> AGENTS.md
└── README.md -> AGENTS.md
```

- Start in [`crates/let-cli/src/commands/`](crates/let-cli/src/commands/) for CLI surface changes, [`crates/let-sdk/src/intelligence/`](crates/let-sdk/src/intelligence/) for evidence contracts/orchestration/persistence, and [`crates/let-sdk/src/pipeline/`](crates/let-sdk/src/pipeline/) for provider-specific capture and enrichment behavior
- [`crates/let-cli/src/registry.rs`](crates/let-cli/src/registry.rs) is the source of truth for the agent-facing tool catalog and global flags
- Treat [`target/`](target/), [`node_modules/`](node_modules/), `$LET_HOME/data`, `$LET_HOME/cache`, and `$LET_HOME/sources` as generated or runtime-owned state

## 3. Stack

| Layer | Choice | Notes |
| --- | --- | --- |
| Runtime | Rust 2024 workspace | three crates: SDK, CLI, TUI; `unsafe` forbidden |
| CLI | `clap` + JSON/Toon envelopes | default stdout is JSON; `--toon` emits the same envelope as Toon |
| TUI | `ratatui` + `crossterm` | local browser surface; keep it aligned with the intelligence DB before expanding it |
| Storage | SQLite via `rusqlite` | one intelligence DB plus one DB per enrichment source |
| HTTP / Parsing | `reqwest`, `serde_json`, `csv`, `zip`, `calamine`, `image` | Rightmove fetch, EPC/Mapbox/Notion, source ingests, media normalization |
| JS Tooling | Bun + Husky + lint-staged | wrappers, hooks, and release/install scripts only; product code is Rust |

## 4. Commands

- `bun install` installs JS tooling and activates the Husky hooks in [`.husky/`](.husky/)
- `cargo run -q -p let-cli -- tools` prints the current tool catalog and global flags from [`crates/let-cli/src/registry.rs`](crates/let-cli/src/registry.rs)
- `cargo run -q -p let-cli -- health` checks config, DB/schema health, source DB presence, credentials, and writable runtime dirs
- `cargo run -q -p let-cli -- search resolve <location>` resolves place names to Rightmove location identifiers
- `cargo run -q -p let-cli -- search discover` discovers Rightmove listing ids; discovery is API-first with HTML fallback unless `search.useApi = false`
- `cargo run -q -p let-cli -- inspect <id-or-url> [--depth quick|standard|deep] [--refresh none|stale|all] [--section ...]` gathers and persists one evidence bundle
- `cargo run -q -p let-cli -- evidence <id> [--section ...]` reads a stored evidence bundle
- `cargo run -q -p let-cli -- verify <id> [--claim all|address|broadband|epc|media|description] [--refresh none|stale|all]` verifies extracted claims against available evidence
- `cargo run -q -p let-cli -- correct address|epc|media|clear ...` records append-only manual corrections for bad listing address, postcode, EPC, or map evidence
- `cargo run -q -p let-cli -- assess save|get ...` stores or reads AI-authored assessment JSON; the CLI does not compute the final recommendation
- `cargo run -q -p let-cli -- sources list|status|build <all|name>` manages enrichment DBs under `$LET_HOME/sources`; progress logs are written to stderr, not stdout
- `cargo run -q -p let-cli -- start [--id <id>] [--section ...]` launches `let-tui` with the same runtime path overrides as the CLI
- `bun run build` performs a release build and installs `let` plus `let-tui` into `${LET_HOME:-${TOOLS_HOME:-$HOME/.tools}/let}`
- `bun run util:check` runs the local completion gate: fmt, clippy, `cargo check`, tests, and the release build/install wrapper

## 5. Architecture

- [`crates/let-cli/src/main.rs`](crates/let-cli/src/main.rs) parses flags, resolves path overrides, dispatches commands, and emits structured envelopes
- [`crates/let-cli/src/commands/`](crates/let-cli/src/commands/) should stay thin; persistence, parsing, enrichment, verification, and evidence shaping belong in `let-sdk`
- [`crates/let-sdk/src/intelligence/service.rs`](crates/let-sdk/src/intelligence/service.rs) orchestrates capture, extraction, address resolution, source facts, claim extraction, claim verification, bundle persistence, and assessment reads/writes
- [`crates/let-sdk/src/intelligence/repository.rs`](crates/let-sdk/src/intelligence/repository.rs) owns the intelligence DB schema and versioning
- [`crates/let-sdk/src/pipeline/fetch/rightmove.rs`](crates/let-sdk/src/pipeline/fetch/rightmove.rs) captures Rightmove `PAGE_MODEL`, preserves raw description evidence, extracts media URLs, and classifies active vs let-agreed vs removed pages
- [`crates/let-sdk/src/pipeline/enrich.rs`](crates/let-sdk/src/pipeline/enrich.rs) joins local postcode, IMD, census, population, income, flood, crime, NaPTAN, and UPRN data; missing source DBs degrade the report instead of aborting most workflows
- [`crates/let-sdk/src/pipeline/score.rs`](crates/let-sdk/src/pipeline/score.rs) is retained for deterministic scoring experiments only; agent assessment is the default decision layer
- Default stdout is exactly one JSON envelope per structured command. `--toon` emits the same envelope as Toon. Progress, warnings, and confirmation prompts go to stderr. There is no supported `--json` or `--text` flag.

## 6. Runtime and State

- Path precedence is CLI flags -> `LET_*_DIR` -> `LET_HOME` -> `TOOLS_HOME/let` -> `~/.tools/let`; see [`crates/let-sdk/src/paths.rs`](crates/let-sdk/src/paths.rs)
- Config and secrets live at `$LET_HOME/data/let.config.toml` and `$LET_HOME/data/.env`; `let health` also assumes `$LET_HOME/data` is writable
- The intelligence DB lives at `$LET_HOME/data/let.db` and stores entities, identifiers, source snapshots, observations, facts, claims, verifications, media assets, evidence bundles, append-only corrections, and assessments
- Installed binaries normally live alongside those dirs at `$LET_HOME/let` and `$LET_HOME/let-tui`
- Media cache entries live under `$LET_HOME/cache/<rightmove-id-or-uuid>/`; normalized filenames include an asset kind, short hashes, and a `-v1.jpg` suffix from [`crates/let-sdk/src/pipeline/fetch/cache.rs`](crates/let-sdk/src/pipeline/fetch/cache.rs)
- Source DBs live under `$LET_HOME/sources/{broadband,postcodes,deprivation,census,population,income,flood,naptan,uprn,crime}.db`
- Source builds accept per-input path or URL override env vars plus optional `*_SHA256` integrity guards, and each built DB gets `source_runs` / `source_inputs` metadata written by [`crates/let-sdk/src/sources/common.rs`](crates/let-sdk/src/sources/common.rs)
- Intelligence DB schema version is `1`; schema-version mismatches are treated as `SCHEMA_MISMATCH`, and the supported repair path is DB recreation through `let inspect <rightmove-id>`, not hand migration
- Environment variables that materially affect behavior: `EPC_API_BEARER_TOKEN` preferred, legacy `EPC_API_EMAIL` + `EPC_API_KEY`, `MAPBOX_ACCESS_TOKEN`, `NOTION_API_KEY`, `NOTION_DATABASE_ID`

## 7. Conventions

- Use Rightmove portal IDs for external-facing command workflows and cache directories; intelligence entity ids use `rightmove:<portal-id>`
- Keep command payloads camelCase and envelope-safe; when adding or renaming fields, update [`crates/let-cli/src/registry.rs`](crates/let-cli/src/registry.rs), tests, and any mirrored schemas
- `search.useApi` controls discovery transport only. `fetch.useApi` is intentionally rejected by [`crates/let-sdk/src/config.rs`](crates/let-sdk/src/config.rs).
- Evidence section statuses are `ok`, `partial`, `degraded`, `blocked`, `skipped`, or `stale`; missing optional sources should degrade the relevant section instead of aborting the whole bundle
- Claim verification statuses are `supported`, `contradicted`, `unknown`, and `insufficientEvidence`; use source-backed explanations rather than bare booleans
- Corrections are evidence, not source rewrites. `correct address`, `correct epc`, and `correct media` preserve Rightmove observations, expose active corrections in `evidence`, and influence only dependent resolved sections.

## 8. Constraints

- Never hand-edit runtime DB files, source DBs, or cache assets; use CLI commands or source builders instead
- High-risk files are [`crates/let-sdk/src/intelligence/repository.rs`](crates/let-sdk/src/intelligence/repository.rs), [`crates/let-sdk/src/intelligence/service.rs`](crates/let-sdk/src/intelligence/service.rs), [`crates/let-sdk/src/pipeline/fetch/rightmove.rs`](crates/let-sdk/src/pipeline/fetch/rightmove.rs), [`crates/let-sdk/src/pipeline/epc.rs`](crates/let-sdk/src/pipeline/epc.rs), [`crates/let-sdk/src/sources/`](crates/let-sdk/src/sources/), and [`crates/let-cli/src/registry.rs`](crates/let-cli/src/registry.rs)
- Rightmove, EPC, Mapbox, Notion, and public source datasets are unstable dependencies. Expect API fallback, removed listings, header/schema drift, and partial enrichment.
- Do not commit `$LET_HOME/data/.env`, evidence exports, cached media, test-generated DBs, or personal search context

## 9. Validation

- Required local gate: `bun run util:check`
- CLI surface, envelope, `--toon`, or tool-registry changes: `cargo test -p let-cli --test cli_integration`
- Intelligence DB schema or repository changes: add/update focused tests and run `cargo test -p let-cli --test cli_integration`
- Discovery, inspect, enrichment, source-build, EPC, Mapbox, media, or verification changes: run relevant crate tests plus at least one targeted manual smoke flow where credentials/network make it practical
- Manual smoke for runtime-path or workflow changes: `cargo run -q -p let-cli -- tools`, `health`, `config show`, `sources list`, `sources status`, `search discover`, `inspect <id> --depth quick`, `evidence <id>`, `verify <id> --claim broadband`, `correct address <id> --postcode <postcode> --note <note>`
- If you touch [`crates/let-cli/src/registry.rs`](crates/let-cli/src/registry.rs), [`crates/let-cli/src/main.rs`](crates/let-cli/src/main.rs), or [`crates/let-cli/src/envelope.rs`](crates/let-cli/src/envelope.rs), verify default JSON mode still emits one stdout line and `--toon` decodes to the same envelope shape
