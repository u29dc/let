# Migration Ledger

Status values: `done`, `blocked`.

## Core (SDK)
- `paths/config/schema/db` contracts: `done`
- scoring pipeline + assessed-score recomputation: `done`
- source-build subsystem (`crates/let-sdk/src/sources/*`): `done`
- fetch/parse/enrich portability hardening: `done`

## CLI
- envelope contract: `done`
- tools registry and command discovery: `done`
- command groups (`config/search/fetch/view/score/assess/export/ops/build/start`): `done`
- legacy delegation removal: `done`

## TUI
- ratatui shell, cyan theme, dense layout, command palette, source monitor: `done`

## Source Builders
- former TS source scripts (`broadband/postcodes/deprivation/census/population/income/flood/naptan/uprn/crime`): `done` in Rust
- `build sources all --jobs 3` orchestration: `done`
- retry/fallback/idempotent ingest policy: `done` (duplicate-row upserts + flood source cached fallback)

## Legacy Cleanup
- `packages/*` TypeScript runtime removed from `main`: `done`
- `scripts/sources/*` and old orchestrators removed: `done`
- package scripts switched to Rust workflow: `done`

## Cross-Project Alignment
- `let` scripts/workflow aligned with `cho` and `fin` Rust conventions: `done`
- `let-tui` header/theme/palette contract aligned with `cho` and `fin`: `done`

## Parity
- command-matrix parity harness runs against `archive` vs `main`: `done`
- extended audit diff artifacts generated under `.tmp/parity/*`: `done`
