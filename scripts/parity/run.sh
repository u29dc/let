#!/usr/bin/env bash
set -euo pipefail

ROOT="$(cd "$(dirname "$0")/../.." && pwd)"
ARCHIVE_WORKTREE="${ARCHIVE_WORKTREE:-${ROOT}-archive}"
MAIN_WORKTREE="${MAIN_WORKTREE:-$ROOT}"
OUT_DIR="${OUT_DIR:-$ROOT/.tmp/parity/run}"

mkdir -p "$OUT_DIR/archive" "$OUT_DIR/main" "$OUT_DIR/diff"

run_cmd() {
  local dir="$1"
  local out_prefix="$2"
  shift 2

  (
    cd "$dir"
    bun run let "$@" --json > "$OUT_DIR/$out_prefix.json" 2> "$OUT_DIR/$out_prefix.stderr" || true
  )
}

normalize_json() {
  local src="$1"
  local dest="$2"
  if command -v jq >/dev/null 2>&1; then
    jq 'if type=="object" and .meta then .meta.elapsed = 0 | . else . end' "$src" > "$dest" 2>/dev/null || cp "$src" "$dest"
  else
    cp "$src" "$dest"
  fi
}

compare_cmd() {
  local name="$1"
  shift

  run_cmd "$ARCHIVE_WORKTREE" "archive/$name" "$@"
  run_cmd "$MAIN_WORKTREE" "main/$name" "$@"

  normalize_json "$OUT_DIR/archive/$name.json" "$OUT_DIR/diff/$name.archive.norm.json"
  normalize_json "$OUT_DIR/main/$name.json" "$OUT_DIR/diff/$name.main.norm.json"

  if diff -u "$OUT_DIR/diff/$name.archive.norm.json" "$OUT_DIR/diff/$name.main.norm.json" > "$OUT_DIR/diff/$name.diff"; then
    echo "PASS $name"
  else
    echo "FAIL $name (see $OUT_DIR/diff/$name.diff)"
  fi
}

compare_cmd tools tools
compare_cmd health health
compare_cmd config-show config show
compare_cmd config-validate config validate

echo "parity run complete: $OUT_DIR"
