# Review — Agent-Native Re-Architecture

**Date**: 2026-02-08
**Verdict**: COMPLETE-BUT-ISSUES

The refactor is functionally complete: all 15 `defineToolCommand` tools are registered, the JSON envelope contract works, the binary builds, quality gates pass (302/0/10), and the `/let` skill file is in place. However, PLAN.md's Stage 6 "clean cut" was not fully executed — two legacy files (`shared-read.ts`, `shared-write.ts`) remain with their own path resolution, and the `view` group still routes legacy `stats`/`regions` subcommands.

---

## PLAN Section Checklist

| Section | Status | Notes |
| ------- | ------ | ----- |
| Cross-platform paths (`paths.ts`) | PASS | Correct precedence, caching, derived paths, dev detection |
| JSON envelope (`envelope.ts`) | PASS | `ok()`, `fail()`, `isJsonMode()`; exit codes 0/1/2 correct |
| Tool registry (`tool.ts`) | PASS | 15 tools via `defineToolCommand()`; `tools`/`health` excluded correctly |
| `tools` command | PASS | Returns sorted catalog of 15 tools, globalFlags present |
| `health` command | PASS | 17 checks, fix arrays with resolved paths, exit 2 when blocked |
| `config show`/`validate` | PASS | Envelope correct; validates via Zod |
| `view list`/`detail` | PASS | Filters, sorting, JSON projection; detail returns full object |
| `score explain`/`compute` | PASS | Breakdown + rescore; deterministic |
| `assess candidates`/`context`/`submit` | PASS | Context bundle with absolute media paths; submit validates |
| `search resolve`/`discover`/`diff` | PASS | Registered; diff is CI-safe (no network) |
| `fetch <ids>` | PASS | Partial success pattern with `failed[]` |
| `ops prune`/`verify` | PASS | JSON mode, dry-run, partial success |
| `export json`/`notion` | PASS | Notion has partial success; json exports path |
| Legacy cleanup (Stage 6) | **INCOMPLETE** | `shared-read.ts`, `shared-write.ts` not removed; `view stats`/`regions` remain |
| Registry drift test (LET-009) | SKIPPED | Deferred; still `[ ]` in TODO.md |
| Fetch/search tests (LET-030) | SKIPPED | Still `[ ]` in TODO.md |
| Skill + docs (Stage 7) | PASS | `/let` skill consolidated; AGENTS.md updated (needs date fix) |
| CI matrix | PASS | Ubuntu + macOS; quality + smoke jobs |
| Contract tests | PASS (partial) | 13 tests cover 12/15 tools; network-dependent tools excluded |
| Binary smoke tests | PASS | 7 tests from arbitrary dir with LET_DATA_DIR override |
| Parity gate | PASS | 13-step E2E loop with seeded fixtures |

---

## Findings

### Blocker

None.

### Major

**M1. `shared-read.ts` not migrated to `@let/core/paths`**
- **File**: `packages/cli/src/commands/shared-read.ts:15-39`
- **PLAN ref**: Stage 6 · Files Removed: "`shared-read.ts` — replaced by `core/paths.ts`"
- **Issue**: Contains its own `resolveRootDir()` using `import.meta.dirname` and `LET_HOME`. Defines `ROOT_DIR`, `CACHE_DIR`, `DATA_DIR`, `LISTINGS_DB_PATH`, `CONFIG_PATH` as module-level constants. Does not use `@let/core/paths`.
- **Impact**: Two parallel path resolution systems. The compiled binary could resolve different paths depending on which code path runs first. Risk of divergence.
- **Imported by**: `shared-write.ts`, `ops/verify.ts`, `ops/prune.ts`, `view/index.ts`, `index.ts` (setupSignalHandlers)
- **New ticket**: LET-047

**M2. `shared-write.ts` not refactored per PLAN**
- **File**: `packages/cli/src/commands/shared-write.ts`
- **PLAN ref**: Stage 6 · Files Removed: "`shared-write.ts` — logic distributed to `fetch/index.ts`"
- **Issue**: Still exists with full `processListing()`, `saveListingsFile()`, `getCachedHtml()`, `cachePageModel()`, `loadConfigOrExit()`, `downloadListingAssets()` functions. Uses `CACHE_DIR`, `CONFIG_PATH`, `LISTINGS_DB_PATH` from `shared-read.ts`.
- **Imported by**: `fetch/index.ts`, `ops/verify.ts`, `ops/prune.ts`
- **New ticket**: LET-048

**M3. `view stats` and `view regions` are legacy subcommands still routed**
- **File**: `packages/cli/src/commands/view/index.ts:366-501`
- **PLAN ref**: Target Architecture · Directory Structure shows only `view/list.ts` and `view/detail.ts`
- **Issue**: `viewStats` and `viewRegions` are defined in `view/index.ts` using plain `defineCommand` (not `defineToolCommand`), not registered in tool registry, but still routable via `let view stats` and `let view regions`. They import from `shared-read.ts`.
- **Impact**: Misleading — `tools --json` shows 2 view commands but 4 are actually routable. AGENTS.md documents `view list|detail|stats|regions` inconsistently.
- **New ticket**: LET-049

### Minor

**m1. AGENTS.md lists deleted skill files**
- **File**: `AGENTS.md:114-116`
- **Issue**: Still references `assess.md` and `summarize.md` in the structure tree, but these files were deleted and consolidated into `let.md`.
- **New ticket**: Addressed in AGENTS.md update below.

**m2. AGENTS.md mentions `view list|detail|stats|regions`**
- **File**: `AGENTS.md:193`
- **Issue**: Lists `stats` and `regions` as current commands, but these are legacy and not in the tool registry.
- **New ticket**: Addressed in AGENTS.md update below.

**m3. Contract tests cover 12/15 tools**
- **File**: `packages/cli/tests/contract.test.ts`
- **Issue**: Missing coverage for `search.resolve`, `search.discover`, `fetch` (network-dependent) and `export.notion` (needs credentials). Acceptable for CI, but noted.
- **Note**: These are tested indirectly via the parity gate for non-network commands. Network commands can only be tested in live E2E.

**m4. Paths tests lack platform-specific coverage**
- **File**: `packages/core/tests/paths.test.ts`
- **Issue**: No tests for XDG defaults, macOS `~/Library` defaults, or `LET_HOME` precedence in installed mode. Only dev mode + explicit overrides tested.
- **Note**: Acceptable since CI runs on both ubuntu and macos, but platform-specific path logic is untested at the unit level.

**m5. `AGENT.md` is a stale tracking file**
- **File**: `AGENT.md` (root)
- **Issue**: Created during earlier sessions, now stale. Shows incomplete checklist from early stages. Should be deleted (TODO.md is the tracker).
- **New ticket**: LET-050

### Nice-to-have

**n1. Registry drift test (LET-009) still deferred**
- PLAN specifies a test that asserts every citty-routed command has a `toolRegistry` entry. Currently deferred because "toolRegistry is empty until Stage 2+ commands use defineToolCommand()". All 15 commands are now registered, so the precondition is met. Should be unblocked.

**n2. Binary smoke test checks only 7/15 tool names**
- **File**: `packages/cli/tests/binary-smoke.test.ts`
- **Issue**: Hardcoded list of 7 expected tool names. Could check all 15 or validate against `toolRegistry` length.

**n3. Parity gate has implicit test ordering dependency**
- **File**: `packages/cli/tests/parity-gate.test.ts`
- **Issue**: Step 12 (assess submit) modifies DB, step 13 reads modified state. Sequential execution is implicit (bun default), not explicit.

---

## Reproduction Commands

```bash
# Verify quality gates pass
bun run util:check

# Verify binary builds and tools/health work
bun run build:cli
bin/let tools --json 2>/dev/null | jq '.data.tools | length'  # expect 15
bin/let health --json 2>/dev/null | jq '.data.status'         # expect "blocked" (no config)

# Verify legacy commands are still routable (issue M3)
bin/let view stats 2>&1  # should run (legacy, not in registry)
bin/let view regions 2>&1  # should run (legacy, not in registry)

# Verify dual path resolution (issue M1)
grep -n 'resolveRootDir\|ROOT_DIR\|CACHE_DIR\|DATA_DIR\|LISTINGS_DB_PATH\|CONFIG_PATH' packages/cli/src/commands/shared-read.ts
grep -n "from '@let/core/paths'" packages/cli/src/commands/shared-read.ts  # expect 0 matches
```
