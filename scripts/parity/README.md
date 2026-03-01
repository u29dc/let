# Parity Harness

Compares archive vs main command contracts for selected JSON-mode commands.

## Usage

```bash
scripts/parity/run.sh
```

Environment overrides:
- `ARCHIVE_WORKTREE` (default: `${ROOT}-archive`)
- `MAIN_WORKTREE` (default: repo root)
- `OUT_DIR` (default: `.tmp/parity/run`)

The harness normalizes volatile `meta.elapsed` fields before diffing.
