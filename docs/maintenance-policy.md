# Maintenance Policy

## Scope
- Keep `let`, `fin`, and `cho` aligned for Rust CLI/TUI platform patterns.
- Preserve command-space compatibility for existing `let` workflows while allowing additive features.

## CLI/Build Workflow
- `bun run build` must build workspace release artifacts and install both binaries:
  - `$LET_HOME/let`
  - `$LET_HOME/let-tui`
- Keep `util:*` command names aligned with sibling Rust projects:
  - `util:format`
  - `util:lint`
  - `util:types`
  - `util:test`
  - `util:build`
  - `util:check`
- Treat `util:check` as the required local quality gate before completion.

## TUI Consistency Contract
- Header pattern: `■ <project> v<version>` on the left, context centered, `cmd+p | ctrl+p command palette` on the right.
- Theme tokens:
  - Accent: cyan
  - Body text: gray
  - Muted/border: dark gray
  - Selection: cyan background with white bold foreground
- Palette trigger and interaction:
  - `cmd+p` / `ctrl+p` opens palette
  - `Esc` closes
  - `Enter` executes selected action

## Parity and Safety
- Maintain archive parity checks for core scoring behavior before changing scorer logic.
- Forbid unsafe code at workspace and crate roots.
- Keep source builders idempotent and resilient to upstream irregularities (duplicates, expired URLs, transient failures).

## Legacy Surface
- Do not reintroduce `packages/*` TypeScript runtime or legacy source scripts on `main`.
- Keep source-build utilities integrated into Rust CLI command-space (`build sources ...`).
