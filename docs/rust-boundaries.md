# Rust Boundary Contract

This file defines crate ownership and public interfaces for the Rust rewrite.

## Crates
- `let-sdk`: all domain logic and data access.
- `let-cli`: command parsing, JSON envelope, command registry, health/tools surfaces.
- `let-tui`: terminal runtime and rendering; calls SDK services via stable interfaces.

## Boundary Rules
1. `let-cli` and `let-tui` must not implement business logic.
2. `let-sdk` is the only crate allowed to access SQLite/source DB internals.
3. Shared command/result contracts live in `let-sdk` and are imported by CLI/TUI.
4. JSON envelope is owned by CLI but must map SDK errors deterministically.
5. Source build orchestrator lives in SDK and is invoked by CLI/TUI.

## Planned Public SDK Interfaces
- `SdkContext`: shared runtime context (paths/config/http/sqlite).
- `Services`:
  - `ConfigService`
  - `SearchService`
  - `FetchService`
  - `ViewService`
  - `ScoreService`
  - `AssessService`
  - `ExportService`
  - `OpsService`
  - `SourceBuildService`

## Stability Rules
- Backward compatibility required for existing command names and payload fields.
- Behavioral parity against `archive` is enforced feature-by-feature.
