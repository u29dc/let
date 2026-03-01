# Migration Ledger

Status values: `pending`, `in_progress`, `done`, `blocked`.

## Core
- `packages/core/src/paths.ts` -> `crates/let-sdk/src/paths.rs` (`pending`)
- `packages/core/src/config/*` -> `crates/let-sdk/src/config/*` (`pending`)
- `packages/core/src/schema/*` -> `crates/let-sdk/src/schema/*` (`pending`)
- `packages/core/src/db/*` -> `crates/let-sdk/src/db/*` (`pending`)
- `packages/core/src/pipeline/fetch/*` -> `crates/let-sdk/src/pipeline/fetch/*` (`pending`)
- `packages/core/src/pipeline/parse/*` -> `crates/let-sdk/src/pipeline/parse/*` (`pending`)
- `packages/core/src/pipeline/enrich/*` -> `crates/let-sdk/src/pipeline/enrich/*` (`pending`)
- `packages/core/src/pipeline/score/*` -> `crates/let-sdk/src/pipeline/score/*` (`pending`)
- `packages/core/src/pipeline/assess/*` -> `crates/let-sdk/src/pipeline/assess/*` (`pending`)
- `packages/core/src/pipeline/view/*` -> `crates/let-sdk/src/pipeline/view/*` (`pending`)
- `packages/core/src/pipeline/output/*` -> `crates/let-sdk/src/pipeline/output/*` (`pending`)

## CLI
- `packages/cli/src/envelope.ts` -> `crates/let-cli/src/envelope.rs` (`pending`)
- `packages/cli/src/tool.ts` -> `crates/let-cli/src/registry.rs` (`pending`)
- `packages/cli/src/main.ts` -> `crates/let-cli/src/main.rs` (`pending`)
- `packages/cli/src/commands/*` -> `crates/let-cli/src/commands/*` (`pending`)

## Source builders
- `scripts/utils.ts` -> `crates/let-sdk/src/sources/runtime/*` (`pending`)
- `scripts/build-sources.ts` -> `crates/let-sdk/src/sources/orchestrator.rs` + `crates/let-cli build sources` (`pending`)
- `scripts/sources/broadband.ts` -> `crates/let-sdk/src/sources/builders/broadband.rs` (`pending`)
- `scripts/sources/postcodes.ts` -> `crates/let-sdk/src/sources/builders/postcodes.rs` (`pending`)
- `scripts/sources/deprivation.ts` -> `crates/let-sdk/src/sources/builders/deprivation.rs` (`pending`)
- `scripts/sources/census.ts` -> `crates/let-sdk/src/sources/builders/census.rs` (`pending`)
- `scripts/sources/population.ts` -> `crates/let-sdk/src/sources/builders/population.rs` (`pending`)
- `scripts/sources/income.ts` -> `crates/let-sdk/src/sources/builders/income.rs` (`pending`)
- `scripts/sources/flood.ts` -> `crates/let-sdk/src/sources/builders/flood.rs` (`pending`)
- `scripts/sources/naptan.ts` -> `crates/let-sdk/src/sources/builders/naptan.rs` (`pending`)
- `scripts/sources/uprn.ts` -> `crates/let-sdk/src/sources/builders/uprn.rs` (`pending`)
- `scripts/sources/crime.ts` -> `crates/let-sdk/src/sources/builders/crime.rs` (`pending`)

## TUI
- New implementation in `crates/let-tui/*` aligned to `~/Git/cho/crates/cho-tui/*` (`pending`)
