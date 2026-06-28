#![forbid(unsafe_code)]

use rusqlite::Connection;

use crate::errors::{ErrorCode, LetError, Result};

pub(crate) const INTELLIGENCE_SCHEMA_VERSION: i32 = 2;

pub(crate) fn init_schema(connection: &Connection) -> Result<()> {
    connection.execute_batch(SCHEMA)?;
    connection.pragma_update(None, "user_version", INTELLIGENCE_SCHEMA_VERSION)?;
    validate_schema(connection)
}

pub(crate) fn validate_schema(connection: &Connection) -> Result<()> {
    let version = read_user_version(connection)?;
    if version != INTELLIGENCE_SCHEMA_VERSION {
        return Err(schema_mismatch(version));
    }
    for (table, columns) in REQUIRED_TABLE_COLUMNS {
        validate_required_columns(connection, table, columns)?;
    }
    Ok(())
}

const REQUIRED_TABLE_COLUMNS: &[(&str, &[&str])] = &[
    ("assessments", &["entity_id", "assessment_json", "saved_at"]),
    (
        "score_results",
        &[
            "entity_id",
            "rightmove_id",
            "scorecard_id",
            "scorecard_version",
            "overall",
            "band",
            "confidence",
            "result_json",
            "computed_at",
        ],
    ),
    (
        "evidence_bundles",
        &[
            "entity_id",
            "rightmove_id",
            "depth",
            "section_statuses_json",
            "bundle_json",
            "generated_at",
        ],
    ),
    (
        "source_snapshots",
        &[
            "id",
            "entity_id",
            "source",
            "source_key",
            "url",
            "captured_at",
            "status",
            "content_hash",
        ],
    ),
    (
        "corrections",
        &[
            "id",
            "entity_id",
            "kind",
            "payload_json",
            "active",
            "affected_sections_json",
            "created_at",
        ],
    ),
    (
        "entities",
        &["id", "entity_type", "created_at", "updated_at"],
    ),
    (
        "entity_identifiers",
        &["entity_id", "provider", "provider_id", "created_at"],
    ),
    (
        "observations",
        &[
            "id",
            "entity_id",
            "namespace",
            "name",
            "value_json",
            "confidence",
            "source",
            "observed_at",
        ],
    ),
    (
        "address_candidates",
        &["id", "entity_id", "source", "label", "confidence"],
    ),
    (
        "address_resolutions",
        &[
            "id",
            "entity_id",
            "status",
            "confidence",
            "reason",
            "resolved_at",
        ],
    ),
    (
        "facts",
        &[
            "id",
            "entity_id",
            "provider",
            "category",
            "name",
            "value_json",
            "source_refs_json",
            "confidence",
            "observed_at",
        ],
    ),
    (
        "claims",
        &[
            "id",
            "entity_id",
            "claim_type",
            "claim_text",
            "value_json",
            "source_ref_json",
            "extracted_at",
        ],
    ),
    (
        "verifications",
        &[
            "id",
            "entity_id",
            "claim_type",
            "status",
            "confidence",
            "explanation",
            "evidence_refs_json",
            "verified_at",
        ],
    ),
    (
        "media_assets",
        &[
            "id",
            "entity_id",
            "provider",
            "kind",
            "remote_url",
            "status",
            "captured_at",
        ],
    ),
];

pub(crate) fn read_user_version(connection: &Connection) -> Result<i32> {
    connection
        .pragma_query_value(None, "user_version", |row| row.get::<_, i32>(0))
        .map_err(Into::into)
}

pub(crate) fn database_has_tables(connection: &Connection) -> Result<bool> {
    let count = connection.query_row(
        "SELECT COUNT(*)
         FROM sqlite_master
         WHERE type = 'table'
           AND name NOT LIKE 'sqlite_%'",
        [],
        |row| row.get::<_, i64>(0),
    )?;
    Ok(count > 0)
}

pub(crate) fn schema_mismatch(version: i32) -> LetError {
    LetError::new(
        ErrorCode::SchemaMismatch,
        format!(
            "intelligence database schema version {version} does not match expected version {INTELLIGENCE_SCHEMA_VERSION}"
        ),
        "move the incompatible database aside and run `let inspect <rightmove-id>` to recreate it",
    )
}

fn validate_required_columns(
    connection: &Connection,
    table: &str,
    required: &[&str],
) -> Result<()> {
    let pragma = format!("PRAGMA table_info({table})");
    let mut statement = connection.prepare(&pragma)?;
    let columns = statement
        .query_map([], |row| row.get::<_, String>(1))?
        .collect::<std::result::Result<Vec<_>, _>>()?;
    if columns.is_empty() {
        return Err(schema_shape_mismatch(format!(
            "intelligence database table `{table}` is missing"
        )));
    }
    let missing = required
        .iter()
        .filter(|column| !columns.iter().any(|existing| existing == **column))
        .copied()
        .collect::<Vec<_>>();
    if !missing.is_empty() {
        return Err(schema_shape_mismatch(format!(
            "intelligence database table `{table}` is missing column(s): {}",
            missing.join(", ")
        )));
    }
    Ok(())
}

fn schema_shape_mismatch(message: String) -> LetError {
    LetError::new(
        ErrorCode::SchemaMismatch,
        message,
        "move the incompatible database aside and run `let inspect <rightmove-id>` to recreate it",
    )
}

const SCHEMA: &str = r#"
CREATE TABLE IF NOT EXISTS entities (
    id TEXT PRIMARY KEY,
    entity_type TEXT NOT NULL,
    created_at TEXT NOT NULL,
    updated_at TEXT NOT NULL
);

CREATE TABLE IF NOT EXISTS entity_identifiers (
    entity_id TEXT NOT NULL REFERENCES entities(id) ON DELETE CASCADE,
    provider TEXT NOT NULL,
    provider_id TEXT NOT NULL,
    url TEXT,
    created_at TEXT NOT NULL,
    PRIMARY KEY (provider, provider_id)
);

CREATE INDEX IF NOT EXISTS idx_entity_identifiers_entity_id
ON entity_identifiers(entity_id);

CREATE TABLE IF NOT EXISTS source_snapshots (
    id TEXT PRIMARY KEY,
    entity_id TEXT NOT NULL REFERENCES entities(id) ON DELETE CASCADE,
    source TEXT NOT NULL,
    source_key TEXT NOT NULL,
    url TEXT NOT NULL,
    captured_at TEXT NOT NULL,
    status TEXT NOT NULL,
    etag TEXT,
    content_hash TEXT NOT NULL,
    raw_json TEXT,
    raw_text TEXT
);

CREATE TABLE IF NOT EXISTS observations (
    id TEXT PRIMARY KEY,
    entity_id TEXT NOT NULL REFERENCES entities(id) ON DELETE CASCADE,
    snapshot_id TEXT REFERENCES source_snapshots(id) ON DELETE SET NULL,
    namespace TEXT NOT NULL,
    name TEXT NOT NULL,
    value_json TEXT NOT NULL,
    confidence TEXT NOT NULL,
    source TEXT NOT NULL,
    observed_at TEXT NOT NULL
);

CREATE TABLE IF NOT EXISTS address_candidates (
    id TEXT PRIMARY KEY,
    entity_id TEXT NOT NULL REFERENCES entities(id) ON DELETE CASCADE,
    source TEXT NOT NULL,
    label TEXT NOT NULL,
    postcode TEXT,
    lat REAL,
    lng REAL,
    confidence TEXT NOT NULL,
    raw_json TEXT
);

CREATE TABLE IF NOT EXISTS address_resolutions (
    id TEXT PRIMARY KEY,
    entity_id TEXT NOT NULL REFERENCES entities(id) ON DELETE CASCADE,
    selected_candidate_id TEXT REFERENCES address_candidates(id) ON DELETE SET NULL,
    status TEXT NOT NULL,
    confidence TEXT NOT NULL,
    reason TEXT NOT NULL,
    resolved_at TEXT NOT NULL
);

CREATE TABLE IF NOT EXISTS facts (
    id TEXT PRIMARY KEY,
    entity_id TEXT NOT NULL REFERENCES entities(id) ON DELETE CASCADE,
    provider TEXT NOT NULL,
    category TEXT NOT NULL,
    name TEXT NOT NULL,
    value_json TEXT NOT NULL,
    source_refs_json TEXT NOT NULL,
    confidence TEXT NOT NULL,
    observed_at TEXT NOT NULL
);

CREATE TABLE IF NOT EXISTS claims (
    id TEXT PRIMARY KEY,
    entity_id TEXT NOT NULL REFERENCES entities(id) ON DELETE CASCADE,
    claim_type TEXT NOT NULL,
    claim_text TEXT NOT NULL,
    value_json TEXT NOT NULL,
    source_ref_json TEXT NOT NULL,
    extracted_at TEXT NOT NULL
);

CREATE TABLE IF NOT EXISTS verifications (
    id TEXT PRIMARY KEY,
    entity_id TEXT NOT NULL REFERENCES entities(id) ON DELETE CASCADE,
    claim_id TEXT,
    claim_type TEXT NOT NULL,
    status TEXT NOT NULL,
    confidence TEXT NOT NULL,
    explanation TEXT NOT NULL,
    evidence_refs_json TEXT NOT NULL,
    verified_at TEXT NOT NULL
);

CREATE TABLE IF NOT EXISTS media_assets (
    id TEXT PRIMARY KEY,
    entity_id TEXT NOT NULL REFERENCES entities(id) ON DELETE CASCADE,
    provider TEXT NOT NULL,
    kind TEXT NOT NULL,
    remote_url TEXT NOT NULL,
    local_path TEXT,
    width INTEGER,
    height INTEGER,
    content_hash TEXT,
    status TEXT NOT NULL,
    captured_at TEXT NOT NULL
);

CREATE TABLE IF NOT EXISTS corrections (
    id TEXT PRIMARY KEY,
    entity_id TEXT NOT NULL REFERENCES entities(id) ON DELETE CASCADE,
    kind TEXT NOT NULL,
    payload_json TEXT NOT NULL,
    note TEXT,
    active INTEGER NOT NULL DEFAULT 1,
    affected_sections_json TEXT NOT NULL,
    created_at TEXT NOT NULL,
    cleared_at TEXT
);

CREATE INDEX IF NOT EXISTS idx_corrections_entity_active
ON corrections(entity_id, active, kind);

CREATE TABLE IF NOT EXISTS evidence_bundles (
    entity_id TEXT PRIMARY KEY REFERENCES entities(id) ON DELETE CASCADE,
    rightmove_id TEXT NOT NULL,
    depth TEXT NOT NULL,
    section_statuses_json TEXT NOT NULL,
    bundle_json TEXT NOT NULL,
    generated_at TEXT NOT NULL
);

CREATE UNIQUE INDEX IF NOT EXISTS idx_evidence_bundles_rightmove_id
ON evidence_bundles(rightmove_id);

CREATE TABLE IF NOT EXISTS assessments (
    entity_id TEXT PRIMARY KEY REFERENCES entities(id) ON DELETE CASCADE,
    assessment_json TEXT NOT NULL,
    saved_at TEXT NOT NULL
);

CREATE TABLE IF NOT EXISTS score_results (
    entity_id TEXT NOT NULL REFERENCES entities(id) ON DELETE CASCADE,
    rightmove_id TEXT NOT NULL,
    scorecard_id TEXT NOT NULL,
    scorecard_version INTEGER NOT NULL,
    overall REAL NOT NULL,
    band TEXT NOT NULL,
    confidence TEXT NOT NULL,
    result_json TEXT NOT NULL,
    computed_at TEXT NOT NULL,
    PRIMARY KEY (entity_id, scorecard_id)
);

CREATE INDEX IF NOT EXISTS idx_score_results_scorecard
ON score_results(scorecard_id, overall DESC, computed_at DESC);
"#;
