#!/usr/bin/env bash
set -euo pipefail

ROOT="$(cd "$(dirname "$0")/../.." && pwd)"
ARCHIVE_WORKTREE="${ARCHIVE_WORKTREE:-${ROOT}-archive}"
MAIN_WORKTREE="${MAIN_WORKTREE:-$ROOT}"
OUT_DIR="${OUT_DIR:-$ROOT/.tmp/parity/run}"

ARCHIVE_EXEC="${ARCHIVE_EXEC:-bun run let}"
MAIN_EXEC="${MAIN_EXEC:-cargo run -q -p let-cli --}"

PARITY_DATA_DIR="${PARITY_DATA_DIR:-}"
PARITY_CONFIG_DIR="${PARITY_CONFIG_DIR:-}"
PARITY_CACHE_DIR="${PARITY_CACHE_DIR:-}"
PARITY_SOURCES_DIR="${PARITY_SOURCES_DIR:-}"

mkdir -p "$OUT_DIR/archive" "$OUT_DIR/main" "$OUT_DIR/diff"

PASS_COUNT=0
FAIL_COUNT=0

build_global_args() {
  local args=()
  if [[ -n "$PARITY_DATA_DIR" ]]; then
    args+=(--data-dir "$PARITY_DATA_DIR")
  fi
  if [[ -n "$PARITY_CONFIG_DIR" ]]; then
    args+=(--config-dir "$PARITY_CONFIG_DIR")
  fi
  if [[ -n "$PARITY_CACHE_DIR" ]]; then
    args+=(--cache-dir "$PARITY_CACHE_DIR")
  fi
  if [[ -n "$PARITY_SOURCES_DIR" ]]; then
    args+=(--sources-dir "$PARITY_SOURCES_DIR")
  fi
  printf '%s\n' "${args[@]}"
}

extract_last_json_line() {
  local src="$1"
  local dest="$2"
  local line
  line="$(grep -E '^\{.*\}$' "$src" | tail -n 1 || true)"
  if [[ -n "$line" ]]; then
    printf '%s\n' "$line" > "$dest"
  else
    cp "$src" "$dest"
  fi
}

run_cmd() {
  local dir="$1"
  local out_prefix="$2"
  local executor="$3"
  shift 3

  local -a global_args=()
  mapfile -t global_args < <(build_global_args)

  (
    cd "$dir"
    # shellcheck disable=SC2086
    eval "$executor" "${global_args[@]}" "$@" --json > "$OUT_DIR/$out_prefix.raw" 2> "$OUT_DIR/$out_prefix.stderr" || true
  )

  extract_last_json_line "$OUT_DIR/$out_prefix.raw" "$OUT_DIR/$out_prefix.json"
}

normalize_json() {
  local src="$1"
  local dest="$2"
  if command -v jq >/dev/null 2>&1; then
    jq -S '
      def normalize_numbers:
        walk(
          if type=="number" then
            if . == floor then floor else (. * 1000 | round / 1000) end
          else
            .
          end
        );
      .
      | if type=="object" and .meta then .meta = { tool: .meta.tool } else . end
      | if type=="object" and .data and (.data|type) == "object" and .data.listings then
          .data.listings |= map(
            del(.portalId, .scoreChange)
            | if has("url") then .url |= gsub("https://www\\.rightmove\\.co\\.uk"; "https://rightmove.co.uk") else . end
          )
        else
          .
        end
      | normalize_numbers
    ' "$src" > "$dest" 2>/dev/null || cp "$src" "$dest"
  else
    cp "$src" "$dest"
  fi
}

compare_cmd() {
  local name="$1"
  shift

  run_cmd "$ARCHIVE_WORKTREE" "archive/$name" "$ARCHIVE_EXEC" "$@"
  run_cmd "$MAIN_WORKTREE" "main/$name" "$MAIN_EXEC" "$@"

  normalize_json "$OUT_DIR/archive/$name.json" "$OUT_DIR/diff/$name.archive.norm.json"
  normalize_json "$OUT_DIR/main/$name.json" "$OUT_DIR/diff/$name.main.norm.json"

  if [[ "$name" == "tools" ]]; then
    if command -v jq >/dev/null 2>&1; then
      local missing
      missing="$(
        jq -n \
          --slurpfile archive "$OUT_DIR/diff/$name.archive.norm.json" \
          --slurpfile main "$OUT_DIR/diff/$name.main.norm.json" \
          '$archive[0].data.tools | map(.name) - ($main[0].data.tools | map(.name))'
      )"
      if [[ "$missing" == "[]" ]]; then
        echo "PASS $name (archive tools subset present in main)"
        PASS_COUNT=$((PASS_COUNT + 1))
        return
      fi
      echo "$missing" > "$OUT_DIR/diff/$name.missing.json"
    fi
  fi

  if diff -u "$OUT_DIR/diff/$name.archive.norm.json" "$OUT_DIR/diff/$name.main.norm.json" > "$OUT_DIR/diff/$name.diff"; then
    echo "PASS $name"
    PASS_COUNT=$((PASS_COUNT + 1))
  else
    echo "FAIL $name (see $OUT_DIR/diff/$name.diff)"
    FAIL_COUNT=$((FAIL_COUNT + 1))
  fi
}

compare_cmd tools tools
compare_cmd health health
compare_cmd config-show config show
compare_cmd config-validate config validate
compare_cmd view-list view list --top 5
compare_cmd assess-candidates assess candidates --top 5
compare_cmd search-diff search diff 170448131,170448132
compare_cmd ops-prune-dry-run ops prune --min-score 50 --dry-run --force

REPORT="$OUT_DIR/report.md"
{
  echo "# Parity Report"
  echo
  echo "- Archive worktree: \`$ARCHIVE_WORKTREE\`"
  echo "- Main worktree: \`$MAIN_WORKTREE\`"
  echo "- Archive executor: \`$ARCHIVE_EXEC\`"
  echo "- Main executor: \`$MAIN_EXEC\`"
  echo "- Pass: $PASS_COUNT"
  echo "- Fail: $FAIL_COUNT"
  echo
  echo "## Diff Artifacts"
  echo "- Directory: \`$OUT_DIR/diff\`"
} > "$REPORT"

echo "parity run complete: $OUT_DIR (pass=$PASS_COUNT fail=$FAIL_COUNT)"
