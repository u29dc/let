---
name: let
description: >-
    Autonomous UK property search pipeline. Discovers rental listings from
    Rightmove, fetches and enriches with EPC/broadband/area data, scores
    algorithmically, and supports deep assessment with photo/map analysis.
    Use when searching for rental properties, comparing areas, or analyzing
    housing options.
compatibility: >-
    Requires bin/let compiled binary (bun run build:cli). Network access for
    Rightmove scraping and EPC API. Optional: Mapbox token for satellite/street
    map views.
allowed-tools: Bash Read Write WebSearch WebFetch
---

## Prerequisites

The CLI binary must be built at `bin/let` (relative to repo root). Build with `bun run build:cli` if missing.

## Health check

Run `bin/let health --json` immediately after setup. If `status == "blocked"`, read `references/init.md` and help the user resolve prerequisites. If `status == "degraded"`, proceed -- enrichment works with lower confidence but the pipeline is functional.

## Protocol

Read `references/protocol.md` for the full operating protocol. Always follow it.

## Self-describing

Run `bin/let tools --json` for the full command catalog with parameters, examples, and output fields. The CLI is self-documenting; use the catalog as the source of truth for available commands and their signatures.

## On-demand references

- Scoring interpretation and adjustment guidance: `references/scoring.md`
- Error codes and recovery actions: `references/errors.md`

## What is included

- Config template at `templates/config.toml` for first-run setup
- All CLI commands: search, fetch, enrich, score, assess, view, export

## What may be missing

- **Source databases** (`.let/sources/` directory): optional. Without them, enrichment runs in degraded mode -- no broadband, IMD, crime, or flood data. Scores have lower confidence but the pipeline works end-to-end. See `references/init.md` for options.
- **Config file**: must be created from the template on first run. The init reference walks through this conversationally.
- **API keys**: EPC key is required for energy rating enrichment; Mapbox token is optional for satellite/street map views.
