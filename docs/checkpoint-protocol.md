# Checkpoint Protocol

## Purpose
Provide deterministic recovery points while running a large rewrite with many commits.

## Rules
1. Create a git tag after each completed stage: `rewrite-stage-<n>-<shortsha>`.
2. Do not modify `archive`; only compare against it.
3. If a stage regresses parity or tests, reset to the latest stage tag and replay only validated commits.
4. Keep `.tmp/PLAN.md` updated before creating each stage tag.

## Stage Checkpoint Commands
```bash
git tag rewrite-stage-<n>-$(git rev-parse --short HEAD)
git push origin rewrite-stage-<n>-$(git rev-parse --short HEAD)
```

## Recovery
```bash
git switch main
git reset --hard <checkpoint-tag>
```

Use hard reset only when explicitly performing approved rollback from this protocol.
