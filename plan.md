# Let Evidence Workflow Improvement Plan

## 1. Purpose

This plan implements the next practical improvements for `let` after real
multi-area home-search use.

The core workflow already works: `inspect` captures Rightmove evidence, EPC,
broadband, source facts, media, and saved agent assessments. The bottlenecks
are now mechanical:

- agents repeat `inspect` and `evidence` manually for several handpicked
  listings;
- saved assessments have inconsistent shapes, which makes later comparison
  awkward;
- important evidence traps are visible only after manual cross-checking;
- photo review is harder than it needs to be without a generated visual
  overview.

The goal is to improve `let` as an evidence layer, not to turn it into a
subjective property-decision engine.

## 2. Product Boundary

`let` should keep doing deterministic, source-backed evidence work:

- capture listing evidence;
- preserve local artifacts;
- expose external-source status;
- highlight mechanical consistency issues;
- persist agent-authored assessments;
- make repeated primitive operations easier to run.

Agents remain responsible for household judgment:

- whether an area feels right;
- whether a school or commute tradeoff is acceptable;
- whether a flat is worth viewing;
- whether rent is justified by the broader family context.

This distinction matters. Evidence flags can say "EPC address does not match"
or "gigabit coverage is 0%"; they should not say "do not move here."

## 3. Current Architecture Findings

### 3.1 CLI Surface

The CLI is intentionally thin:

- `crates/let-cli/src/main.rs` parses clap commands, resolves runtime paths,
  dispatches command wrappers, and emits one JSON or Toon envelope.
- `crates/let-cli/src/commands/inspect.rs` accepts one `id_or_url` and calls
  `let_sdk::intelligence::IntelligenceService::inspect`.
- `crates/let-cli/src/commands/evidence.rs` accepts one `id` and reads one
  bundle through the SDK.
- `crates/let-cli/src/commands/agent_assess.rs` saves and lists arbitrary JSON
  assessment objects.
- `crates/let-cli/src/registry.rs` is the agent-facing command catalog and must
  stay aligned with executable behavior.

The batch work should mostly live in the CLI command wrappers because it is
repeating the same primitive over multiple inputs. The underlying SDK primitive
should stay one-listing oriented unless a real shared domain operation appears.

### 3.2 Evidence Model

`crates/let-sdk/src/intelligence/types.rs` defines the public bundle shape.
`EvidenceBundle` already has:

- listing identity;
- Rightmove evidence;
- address evidence;
- EPC evidence;
- broadband evidence;
- source facts;
- media evidence;
- claims and verifications;
- saved assessment;
- corrections;
- section states;
- next actions.

It does not yet have a first-class generic evidence-quality signal, so agents
have to infer warning conditions by reading several unrelated fields.

### 3.3 Assessment Model

Assessments are intentionally agent-authored JSON. That should remain true:
there is no reason to reject useful future fields or force all agents into one
strict schema.

The missing layer is a normalized comparison view over common fields. The raw
assessment can stay flexible while the CLI exposes stable recommended keys for
`assess get`, `assess list`, and bundle evidence.

### 3.4 Media Pipeline

`crates/let-sdk/src/pipeline/fetch/media.rs` downloads, validates, normalizes,
hashes, and caches Rightmove photos, floorplans, EPC graphs, and map images.
`crates/let-sdk/src/pipeline/fetch/cache.rs` owns deterministic cache naming.

This is the right layer for contact sheets because a contact sheet is a derived
media artifact. It should be created as part of media inspection when local
photos exist, not as a separate subjective analysis command.

### 3.5 TUI Media Review

`crates/let-tui` builds a media list from local bundle/cache state. Contact
sheets should appear as a normal media artifact so the existing preview and
quicklook path can handle them. The TUI should not need to understand how the
sheet was generated.

## 4. Planned Changes

## 4.1 Multi-Input `inspect`

### User Contract

Support all of these forms:

```sh
let inspect 89540679
let inspect 89540679 89385534 89660973 --depth deep --refresh all
let inspect $(cat ids.txt) --depth standard
cat ids.txt | let inspect --depth quick
let inspect --depth quick < ids.txt
```

There will be no `--stdin` flag. Input detection should be automatic:

1. If one or more positional IDs or URLs are present, use them.
2. If no positional IDs or URLs are present and stdin is not a terminal, read
   stdin to EOF.
3. Split stdin on whitespace, commas, and newlines.
4. If no inputs remain, return a normal validation error.

Notes for shell behavior:

- `$(cat ids.txt)` expands file contents into positional arguments.
- `cat ids.txt | let inspect` and `let inspect < ids.txt` use stdin.
- `>` is stdout redirection and is not a list-input syntax.
- `||` is shell OR and is not a list-input syntax.

### Output Contract

Single input remains backward compatible:

- `let inspect 89540679` returns the current single-listing payload.

Batch input returns one envelope whose `data.items[]` preserves input order:

```json
{
  "items": [
    {
      "input": "89540679",
      "id": "89540679",
      "ok": true,
      "bundle": {},
      "elapsed": "1.23s",
      "warnings": []
    },
    {
      "input": "bad-id",
      "id": null,
      "ok": false,
      "error": {
        "code": "invalid_input",
        "message": "invalid Rightmove id or URL",
        "hint": "pass a Rightmove portal id or listing URL"
      },
      "elapsed": "0.00s",
      "warnings": []
    }
  ],
  "count": 2,
  "okCount": 1,
  "errorCount": 1
}
```

The command exits `0` when the batch runner itself succeeded, even if individual
items have `ok: false`. A total setup failure, such as no input, still returns
the normal CLI error envelope and non-zero exit.

### Implementation Details

- Change the clap positional from required `String` to optional variadic
  `Vec<String>` with `num_args = 0..`; otherwise stdin mode is rejected before
  command dispatch.
- Add a small shared input resolver in `let-cli`, used by both `inspect` and
  `evidence`.
- Use `std::io::IsTerminal` to detect whether stdin should be read.
- Split tokens conservatively; do not parse URLs with ad hoc string surgery
  beyond existing Rightmove-id normalization.
- Run items sequentially for now. This avoids Rightmove rate spikes, SQLite
  contention, and noisy progress interleaving.
- Measure per-item elapsed time in the CLI wrapper.
- Keep SDK `inspect` single-listing oriented.

## 4.2 Multi-Input `evidence`

### User Contract

Support:

```sh
let evidence 89540679
let evidence 89540679 89385534 89660973 --section rightmove,epc,broadband,media,assessment
cat ids.txt | let evidence --section media,assessment
```

The existing `let evidence list` special case remains only when `list` is the
sole positional argument. `list` mixed with other IDs is invalid.

### Output Contract

Single input remains backward compatible.

Batch output mirrors `inspect`:

```json
{
  "items": [
    {
      "input": "89540679",
      "id": "89540679",
      "ok": true,
      "bundle": {},
      "elapsed": "0.02s",
      "warnings": []
    }
  ],
  "count": 1,
  "okCount": 1,
  "errorCount": 0
}
```

Warnings should include bundle evidence flags and any high-signal section
warnings already present in the bundle. These are request-level batch warnings;
the persisted bundle flags stay derived from the bundle itself and should not
change merely because `evidence --section` was narrower than the stored bundle.

## 4.3 Assessment Normalization

### User Contract

`assess save` continues accepting arbitrary JSON. This remains valid:

```sh
let assess save 89540679 '{"anything":"useful"}'
```

When common fields are present, the CLI exposes a normalized assessment view:

```json
{
  "assessment": {
    "recommendation": "view",
    "confidence": "medium_high",
    "summary": "Strong practical candidate with some EPC questions.",
    "positives": ["fast train access"],
    "risks": ["EPC address mismatch"],
    "nextActions": ["verify floor area at viewing"],
    "tradeoffs": ["smaller garden for easier commute"],
    "areaNotes": "Walkable center, but check nursery route.",
    "commuteNotes": "Good London bridge route.",
    "familyFit": "Plausible if storage works.",
    "evidenceGaps": ["confirm EPC match"],
    "source": "agent"
  }
}
```

Recommended enum values:

- `recommendation`: `view`, `stretch_view`, `backup_view`, `watch`, `pass`,
  `benchmark`
- `confidence`: `high`, `medium_high`, `medium`, `low`

### Output Shape

Backward-compatible shape:

- `assess save` and `assess get` continue returning an `AssessmentRecord` where
  `data.assessment` is the raw stored JSON.
- `AssessmentRecord` gains `normalizedAssessment`, so callers get
  `data.normalizedAssessment` next to the raw `data.assessment`.
- Evidence bundles keep the existing `bundle.assessment.assessment` raw nested
  object and add `bundle.assessment.normalizedAssessment`.
- `assessment` is never replaced by the normalized view.

`assess list` should expose comparison-friendly columns:

- `id`;
- `recommendation`;
- `confidence`;
- `summary`;
- `positives`;
- `risks`;
- `nextActions`;
- `tradeoffs`;
- `areaNotes`;
- `commuteNotes`;
- `familyFit`;
- `evidenceGaps`;
- `source`;
- `assessment` raw object.

### Implementation Details

- Add a small `NormalizedAssessment` struct in `let-sdk` intelligence types.
- Add a normalizer helper that accepts a raw `serde_json::Value`.
- Treat missing fields as `null` or empty arrays.
- Accept simple scalar or string-array variants for list fields where practical.
- Normalize camelCase and snake_case aliases for `nextActions` /
  `next_actions` and `evidenceGaps` / `evidence_gaps`.
- Do not reject saves with unknown enum values; expose a warning in the
  normalized view instead.
- `assess list --recommendation` and `--confidence` should filter using
  normalized values first, with raw fallback only for compatibility.

## 4.4 Generic Evidence-Quality Flags

### User Contract

Bundles gain `flags[]`:

```json
{
  "flags": [
    {
      "severity": "warning",
      "category": "source_conflict",
      "code": "epc_address_mismatch",
      "summary": "The selected EPC certificate address does not match the resolved listing address.",
      "sources": ["epcApi", "rightmove"],
      "recommendedAction": "verify the EPC certificate at viewing or record a correction"
    }
  ]
}
```

Categories:

- `missing_required_evidence`;
- `source_conflict`;
- `source_degraded`;
- `suspicious_value`;
- `manual_verification_needed`.

Severities:

- `info`;
- `warning`;
- `critical`.

### Initial Flag Rules

Implement generic, source-backed flags only:

- `missing_epc`: EPC section requested or expected but EPC evidence is missing,
  blocked, or degraded.
- `epc_address_mismatch`: selected EPC certificate says the address does not
  match.
- `suspicious_floor_area`: EPC floor area is implausible for the captured
  listing shape. Initial exact thresholds: flats with 0-2 bedrooms over
  150 sqm, houses with 0-2 bedrooms over 220 sqm, or any home over 500 sqm.
- `low_gigabit_coverage`: broadband section reports no or very low gigabit
  availability.
- `partial_gigabit_coverage`: broadband section reports partial gigabit
  availability.
- `media_degraded`: media section requested but no local photos were cached, or
  media evidence is degraded.
- `address_degraded`: resolved address/postcode confidence is degraded or
  missing.
- `source_section_degraded`: any major evidence section is degraded or blocked.
- `high_crime_rate`: a source fact named `crimeRatePer1k` is present and is at
  least `150.0`; this threshold is intentionally conservative and should be
  revisited only with documented source distribution evidence.
- `active_manual_correction`: an active correction is influencing address, EPC,
  or media evidence and should be remembered as manual evidence.

Do not implement floorplan-vs-EPC area conflict until `let` has a reliable
floorplan-area extraction source. The plan should reserve the code
`epc_floor_area_conflict`, but no flag should be emitted from guessed OCR.

### Implementation Details

- Add `EvidenceQualityFlag` to `let-sdk` intelligence types.
- Add `flags: Vec<EvidenceQualityFlag>` to `EvidenceBundle` with serde default
  and skip-empty behavior.
- Compute flags after bundle assembly and before persistence.
- Recompute flags deterministically on every bundle load and immediately before
  save. A serde-default empty vector cannot distinguish "old bundle missing
  flags" from "new bundle has no flags", so read-time recomputation is simpler
  and safer than optional deserialization.
- Include relevant flags in batch item `warnings`.
- Keep flags deterministic and explainable; every flag must cite source names.

## 4.5 Automatic Contact Sheet Artifact

### User Contract

When `inspect` includes media and local photos are cached, media evidence gains:

```json
{
  "media": {
    "contactSheet": {
      "status": "generated",
      "localPath": "/.../89540679-contact-sheet-v1.jpg",
      "photoCount": 12,
      "generatedAt": "2026-06-20T12:34:56Z",
      "width": 1200,
      "height": 900,
      "contentHash": "..."
    }
  }
}
```

If no local photos are available:

```json
{
  "media": {
    "contactSheet": {
      "status": "skipped",
      "photoCount": 0
    }
  }
}
```

If generation fails, inspect should still succeed and expose a degraded artifact
status plus an evidence-quality flag.

### Implementation Details

- Generate contact sheets in `crates/let-sdk/src/pipeline/fetch/media.rs`
  after photo normalization.
- Use cached local photo files only; never re-fetch remote photos for the sheet.
- Cap the sheet to a useful number of photos, initially 16.
- Use a deterministic grid:
  - 4 columns for 9 or more photos;
  - 3 columns for fewer;
  - fixed thumbnail cells;
  - fit images into cells without cropping;
  - white or neutral background;
  - small consistent gutters.
- Write atomically through a temporary file and rename.
- Use a deterministic cache filename such as
  `<listing-id>-contact-sheet-v1.jpg`.
- Reuse existing image crate dependencies; do not add a new rendering stack.
- Store path, dimensions, content hash, photo count, status, and generated time
  in media evidence.
- Persist the derived artifact through the existing `media_assets` table by
  serializing it as kind `contactSheet` with a synthetic local-only remote URL
  such as `local://contact-sheet/<entity-id>`. This avoids a schema migration
  while making the artifact visible to existing media inventory code.

### TUI Integration

- Add the contact sheet as a normal media item named `contact-sheet` by reading
  `bundle.media.contactSheet.localPath` in the bundle-to-TUI projection.
- Prefer showing it before individual photos in the media pane.
- Let the existing preview/quicklook path open the local JPEG.
- Do not add a separate TUI mode for contact sheets.

## 4.6 Registry, Docs, And Agent Skill Updates

Update the agent-facing catalog:

- `inspect` positional input should describe one or more IDs/URLs plus automatic
  stdin when no positional input is present.
- `evidence` should mirror the multi-input contract.
- Output fields should include `items[]` for batch mode, `flags[]`,
  `normalizedAssessment`, and `media.contactSheet`.
- Examples should include both one-listing and multi-listing forms.

Update `.claude/skills/let/SKILL.md` only where it gives agents durable command
guidance:

- batch mechanical loops can be replaced by multi-input commands;
- saved assessments should prefer the recommended envelope;
- flags are warnings, not final judgment.

## 5. Testing Plan

### Unit And Integration Tests

Add or update tests for:

- positional multi-ID parsing for `inspect`;
- automatic stdin parsing for `inspect`;
- positional multi-ID parsing for `evidence`;
- `evidence list` remaining valid as a sole special case;
- `evidence list` mixed with IDs returning a validation error;
- single-input compatibility for `inspect` and `evidence`;
- batch output including per-item error objects and counts;
- assessment normalization for camelCase, snake_case, missing fields, invalid
  enum values, scalar-to-array coercion, and list output;
- evidence flags for missing/degraded sections, EPC address mismatch,
  suspicious values, broadband warnings, media degradation, and manual
  corrections;
- contact sheet generation from local image fixtures;
- contact sheet skipped/failed states;
- old bundle JSON deserialization with new `flags`, `normalizedAssessment`, and
  `contactSheet` fields absent;
- `media_assets` persistence of contact sheets;
- TUI media ordering with contact sheet before photos;
- TUI media list includes contact sheet when present.

### CLI Contract Tests

For touched command surfaces:

- default JSON emits one envelope on stdout;
- errors remain structured;
- Toon output decodes to the same envelope shape;
- `let tools` advertises the new behavior.

### Runtime Smoke Tests

Use an isolated runtime where practical:

```sh
LET_HOME="$(mktemp -d)" cargo run -q -p let-cli -- tools
LET_HOME="$(mktemp -d)" cargo run -q -p let-cli -- health
LET_HOME="$(mktemp -d)" cargo run -q -p let-cli -- search resolve London
```

For live network smoke, discover a small number of listings and run:

```sh
let inspect <id1> <id2> --depth quick --refresh stale
printf '%s\n' <id1> <id2> | let inspect --depth quick
let evidence <id1> <id2> --section rightmove,epc,broadband,media,assessment
let assess save <id1> '{...recommended fields...}'
let assess get <id1>
let assess list
```

If source DBs or credentials are unavailable in the isolated runtime, record
the degraded checks explicitly rather than hiding them.

### Required Gate

Final validation must include:

```sh
cargo fmt --all -- --check
cargo clippy --workspace --all-targets --all-features -- -D warnings
cargo check --workspace --all-targets
cargo test --workspace
bunx tsc --noEmit
bun run util:check
git diff --check
```

`bun run util:check` intentionally performs the release build/install flow.

## 6. Review Plan

Before implementation:

- run an independent plan review against the product boundary;
- check for overbuilt subjective judgment;
- check for CLI contract breaks;
- check for missing tests and migration risks;
- update this plan before coding.

After implementation:

- run an independent code review against the final diff;
- inspect risky areas manually:
  - CLI parsing and stdin behavior;
  - envelope compatibility;
  - bundle serialization;
  - media cache/contact-sheet writes;
  - assessment normalization;
  - TUI media list behavior;
- run the full validation gate and live smoke flows.

## 7. Non-Goals

Do not build:

- a full area-profile command;
- a subjective recommendation engine;
- a bespoke `--urls-file` command path;
- OCR-based floorplan area conflict detection without a reliable extraction
  source;
- a native comparison table before batch `evidence` and normalized
  `assess list` prove insufficient;
- broad DB migrations unless additive serialization fields cannot solve the
  problem.

## 8. Expected Outcome

After implementation, an agent should be able to:

1. discover candidate listings;
2. inspect several selected listings with one command;
3. read several stored evidence bundles with one command;
4. see consistent assessment fields across saved results;
5. notice mechanical evidence traps through `flags[]`;
6. open a contact sheet quickly in the TUI or from the evidence bundle;
7. keep making the final family/property judgment outside the tool.
