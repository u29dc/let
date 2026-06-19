#![forbid(unsafe_code)]

use std::fs;
use std::path::Path;
use std::time::Duration;

use rusqlite::{Connection, OptionalExtension, params};
use serde::Serialize;
use serde::de::DeserializeOwned;

use crate::errors::{ErrorCode, LetError, Result};
use crate::intelligence::types::{
    AssessmentRecord, CorrectionKind, CorrectionRecord, EvidenceBundle,
};
use crate::utils::time::now_iso;

const INTELLIGENCE_SCHEMA_VERSION: i32 = 1;
const SQLITE_BUSY_TIMEOUT: Duration = Duration::from_millis(5_000);

pub struct IntelligenceDb {
    connection: Connection,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct IntelligenceDbOverview {
    pub path: String,
    pub schema_version: i32,
    pub entity_count: i64,
    pub bundle_count: i64,
    pub assessment_count: i64,
}

impl IntelligenceDb {
    pub fn open(path: impl AsRef<Path>) -> Result<Self> {
        let path = path.as_ref();
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)?;
        }
        let connection = Connection::open(path)?;
        connection.busy_timeout(SQLITE_BUSY_TIMEOUT)?;
        let version = read_user_version(&connection)?;
        if version == INTELLIGENCE_SCHEMA_VERSION {
            ensure_additive_schema(&connection)?;
            validate_schema(&connection)?;
        } else if version == 0 && !database_has_tables(&connection)? {
            init_schema(&connection)?;
        } else {
            return Err(schema_mismatch(version));
        }
        Ok(Self { connection })
    }

    pub fn open_readonly(path: impl AsRef<Path>) -> Result<Self> {
        let connection =
            Connection::open_with_flags(path, rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY)?;
        connection.busy_timeout(SQLITE_BUSY_TIMEOUT)?;
        validate_schema(&connection)?;
        Ok(Self { connection })
    }

    pub fn load_bundle(&self, id: &str) -> Result<Option<EvidenceBundle>> {
        let entity_id = normalize_entity_id(id);
        let mut statement = self.connection.prepare(
            "SELECT bundle_json
             FROM evidence_bundles
             WHERE entity_id = ?1 OR rightmove_id = ?2
             LIMIT 1",
        )?;

        let raw = statement
            .query_row(params![entity_id, id], |row| row.get::<_, String>(0))
            .optional()?;
        raw.map(|value| self.deserialize_bundle(&value)).transpose()
    }

    pub fn load_bundles(&self) -> Result<Vec<EvidenceBundle>> {
        let mut statement = self.connection.prepare(
            "SELECT bundle_json
             FROM evidence_bundles
             ORDER BY generated_at DESC",
        )?;
        let rows = statement.query_map([], |row| row.get::<_, String>(0))?;
        let mut bundles = Vec::new();
        for row in rows {
            bundles.push(self.deserialize_bundle(&row?)?);
        }
        Ok(bundles)
    }

    fn deserialize_bundle(&self, raw: &str) -> Result<EvidenceBundle> {
        let mut bundle = deserialize_json::<EvidenceBundle>(raw, "evidence bundle")?;
        bundle.corrections = self.load_active_corrections(&bundle.entity_id)?;
        Ok(bundle)
    }

    pub fn save_bundle(&mut self, bundle: &EvidenceBundle) -> Result<()> {
        let tx = self.connection.transaction()?;
        let now = now_iso();

        tx.execute(
            "INSERT INTO entities (id, entity_type, created_at, updated_at)
             VALUES (?1, 'property_listing', ?2, ?2)
             ON CONFLICT(id) DO UPDATE SET updated_at = excluded.updated_at",
            params![bundle.entity_id, now],
        )?;

        tx.execute(
            "INSERT INTO entity_identifiers (entity_id, provider, provider_id, url, created_at)
             VALUES (?1, 'rightmove', ?2, ?3, ?4)
             ON CONFLICT(provider, provider_id) DO UPDATE
             SET entity_id = excluded.entity_id, url = excluded.url",
            params![bundle.entity_id, bundle.rightmove_id, bundle.url, now],
        )?;

        tx.execute(
            "INSERT INTO evidence_bundles
                 (entity_id, rightmove_id, depth, section_statuses_json, bundle_json, generated_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6)
             ON CONFLICT(entity_id) DO UPDATE SET
                 rightmove_id = excluded.rightmove_id,
                 depth = excluded.depth,
                 section_statuses_json = excluded.section_statuses_json,
                 bundle_json = excluded.bundle_json,
                 generated_at = excluded.generated_at",
            params![
                bundle.entity_id,
                bundle.rightmove_id,
                bundle.depth.as_str(),
                serialize_json(&bundle.sections, "section statuses")?,
                serialize_json(bundle, "evidence bundle")?,
                bundle.generated_at,
            ],
        )?;

        for snapshot in &bundle.source_snapshots {
            tx.execute(
                "INSERT INTO source_snapshots
                     (id, entity_id, source, source_key, url, captured_at, status, content_hash, raw_json)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)
                 ON CONFLICT(id) DO UPDATE SET
                     captured_at = excluded.captured_at,
                     status = excluded.status,
                     content_hash = excluded.content_hash,
                     raw_json = excluded.raw_json",
                params![
                    snapshot.id,
                    bundle.entity_id,
                    snapshot.source,
                    snapshot.source_key,
                    snapshot.url,
                    snapshot.captured_at,
                    snapshot.status,
                    snapshot.content_hash,
                    serialize_json(&snapshot.raw_json, "source snapshot raw JSON")?,
                ],
            )?;
        }

        tx.execute(
            "DELETE FROM facts WHERE entity_id = ?1",
            params![bundle.entity_id],
        )?;
        for fact in &bundle.facts {
            tx.execute(
                "INSERT INTO facts
                     (id, entity_id, provider, category, name, value_json, source_refs_json, confidence, observed_at)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)",
                params![
                    stable_row_id(&[
                        &bundle.entity_id,
                        &format!("{:?}", fact.provider),
                        &fact.category,
                        &fact.name,
                    ]),
                    bundle.entity_id,
                    format!("{:?}", fact.provider),
                    fact.category,
                    fact.name,
                    serialize_json(&fact.value, "fact value")?,
                    serialize_json(&fact.sources, "fact sources")?,
                    format!("{:?}", fact.confidence),
                    bundle.generated_at,
                ],
            )?;
        }

        tx.execute(
            "DELETE FROM claims WHERE entity_id = ?1",
            params![bundle.entity_id],
        )?;
        for claim in &bundle.claims {
            tx.execute(
                "INSERT INTO claims
                     (id, entity_id, claim_type, claim_text, value_json, source_ref_json, extracted_at)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
                params![
                    claim.id,
                    bundle.entity_id,
                    claim.claim_type,
                    claim.claim_text,
                    serialize_json(&claim.value, "claim value")?,
                    serialize_json(&claim.source, "claim source")?,
                    bundle.generated_at,
                ],
            )?;
        }

        tx.execute(
            "DELETE FROM verifications WHERE entity_id = ?1",
            params![bundle.entity_id],
        )?;
        for verification in &bundle.verifications {
            tx.execute(
                "INSERT INTO verifications
                     (id, entity_id, claim_id, claim_type, status, confidence, explanation, evidence_refs_json, verified_at)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)",
                params![
                    verification.id,
                    bundle.entity_id,
                    verification.claim_id,
                    verification.claim_type,
                    format!("{:?}", verification.status),
                    format!("{:?}", verification.confidence),
                    verification.explanation,
                    serialize_json(&verification.evidence, "verification evidence")?,
                    bundle.generated_at,
                ],
            )?;
        }

        tx.execute(
            "DELETE FROM media_assets WHERE entity_id = ?1",
            params![bundle.entity_id],
        )?;
        for item in bundle
            .media
            .photos
            .iter()
            .chain(bundle.media.floorplans.iter())
            .chain(bundle.media.epc_graphs.iter())
            .chain(bundle.media.maps.iter())
        {
            tx.execute(
                "INSERT INTO media_assets
                     (id, entity_id, provider, kind, remote_url, local_path, width, height, content_hash, status, captured_at)
                 VALUES (?1, ?2, 'rightmove', ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)",
                params![
                    stable_row_id(&[&bundle.entity_id, &item.kind, &item.remote_url]),
                    bundle.entity_id,
                    item.kind,
                    item.remote_url,
                    item.local_path,
                    item.width.map(i64::from),
                    item.height.map(i64::from),
                    item.content_hash,
                    item.status,
                    bundle.generated_at,
                ],
            )?;
        }

        tx.commit()?;
        Ok(())
    }

    pub fn save_correction(
        &self,
        entity_id: &str,
        kind: CorrectionKind,
        payload: serde_json::Value,
        note: Option<String>,
        affected_sections: Vec<String>,
    ) -> Result<CorrectionRecord> {
        let normalized = normalize_entity_id(entity_id);
        let now = now_iso();
        let record = CorrectionRecord {
            id: uuid::Uuid::new_v4().to_string(),
            entity_id: normalized,
            kind,
            payload,
            note,
            active: true,
            created_at: now,
            cleared_at: None,
            affected_sections,
        };

        self.connection.execute(
            "INSERT INTO entities (id, entity_type, created_at, updated_at)
             VALUES (?1, 'property_listing', ?2, ?2)
             ON CONFLICT(id) DO UPDATE SET updated_at = excluded.updated_at",
            params![record.entity_id, record.created_at],
        )?;
        self.connection.execute(
            "INSERT INTO corrections
                 (id, entity_id, kind, payload_json, note, active, affected_sections_json, created_at, cleared_at)
             VALUES (?1, ?2, ?3, ?4, ?5, 1, ?6, ?7, NULL)",
            params![
                record.id,
                record.entity_id,
                record.kind.as_str(),
                serialize_json(&record.payload, "correction payload")?,
                record.note,
                serialize_json(&record.affected_sections, "correction affected sections")?,
                record.created_at,
            ],
        )?;
        Ok(record)
    }

    pub fn clear_correction(
        &self,
        entity_id: &str,
        kind: CorrectionKind,
        correction_id: &str,
    ) -> Result<Option<CorrectionRecord>> {
        let normalized = normalize_entity_id(entity_id);
        let now = now_iso();
        let updated = self.connection.execute(
            "UPDATE corrections
             SET active = 0, cleared_at = ?1
             WHERE id = ?2 AND entity_id = ?3 AND kind = ?4",
            params![now, correction_id, normalized, kind.as_str()],
        )?;
        if updated == 0 {
            return Ok(None);
        }
        self.load_correction(correction_id)
    }

    pub fn load_active_corrections(&self, entity_id: &str) -> Result<Vec<CorrectionRecord>> {
        if !table_exists(&self.connection, "corrections")? {
            return Ok(Vec::new());
        }
        let normalized = normalize_entity_id(entity_id);
        let mut statement = self.connection.prepare(
            "SELECT id, entity_id, kind, payload_json, note, active, affected_sections_json, created_at, cleared_at
             FROM corrections
             WHERE entity_id = ?1 AND active = 1
             ORDER BY created_at ASC",
        )?;
        let rows = statement.query_map(params![normalized], correction_from_row)?;
        rows.collect::<std::result::Result<Vec<_>, _>>()
            .map_err(Into::into)
    }

    fn load_correction(&self, correction_id: &str) -> Result<Option<CorrectionRecord>> {
        self.connection
            .query_row(
                "SELECT id, entity_id, kind, payload_json, note, active, affected_sections_json, created_at, cleared_at
                 FROM corrections
                 WHERE id = ?1",
                params![correction_id],
                correction_from_row,
            )
            .optional()
            .map_err(Into::into)
    }

    pub fn save_assessment(
        &self,
        entity_id: &str,
        assessment: serde_json::Value,
    ) -> Result<AssessmentRecord> {
        let normalized = normalize_entity_id(entity_id);
        let record = AssessmentRecord {
            entity_id: normalized,
            assessment,
            saved_at: now_iso(),
        };
        self.connection.execute(
            "INSERT INTO entities (id, entity_type, created_at, updated_at)
             VALUES (?1, 'property_listing', ?2, ?2)
             ON CONFLICT(id) DO UPDATE SET updated_at = excluded.updated_at",
            params![record.entity_id, record.saved_at],
        )?;
        self.connection.execute(
            "INSERT INTO assessments (entity_id, assessment_json, saved_at)
             VALUES (?1, ?2, ?3)
             ON CONFLICT(entity_id) DO UPDATE SET
                 assessment_json = excluded.assessment_json,
                 saved_at = excluded.saved_at",
            params![
                record.entity_id,
                serialize_json(&record.assessment, "assessment")?,
                record.saved_at,
            ],
        )?;
        Ok(record)
    }

    pub fn load_assessment(&self, entity_id: &str) -> Result<Option<AssessmentRecord>> {
        let normalized = normalize_entity_id(entity_id);
        let raw = self
            .connection
            .query_row(
                "SELECT assessment_json, saved_at FROM assessments WHERE entity_id = ?1",
                params![normalized],
                |row| Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?)),
            )
            .optional()?;

        raw.map(|(assessment_json, saved_at)| {
            Ok(AssessmentRecord {
                entity_id: normalize_entity_id(entity_id),
                assessment: deserialize_json(&assessment_json, "assessment")?,
                saved_at,
            })
        })
        .transpose()
    }
}

pub fn database_overview(path: impl AsRef<Path>) -> Result<IntelligenceDbOverview> {
    let path = path.as_ref();
    let db = IntelligenceDb::open_readonly(path)?;
    let schema_version = db
        .connection
        .pragma_query_value(None, "user_version", |row| row.get::<_, i32>(0))?;
    Ok(IntelligenceDbOverview {
        path: path.display().to_string(),
        schema_version,
        entity_count: count_rows(&db.connection, "entities")?,
        bundle_count: count_rows(&db.connection, "evidence_bundles")?,
        assessment_count: count_rows(&db.connection, "assessments")?,
    })
}

pub fn normalize_entity_id(id: &str) -> String {
    let trimmed = id.trim();
    if let Some(rightmove_id) = extract_rightmove_id(trimmed) {
        return format!("rightmove:{rightmove_id}");
    }
    trimmed.to_owned()
}

pub fn extract_rightmove_id(id_or_url: &str) -> Option<String> {
    let trimmed = id_or_url.trim();
    if trimmed.is_empty() {
        return None;
    }
    if let Some(raw) = trimmed.strip_prefix("rightmove:") {
        let digits = raw
            .chars()
            .take_while(|ch| ch.is_ascii_digit())
            .collect::<String>();
        return (!digits.is_empty()).then_some(digits);
    }
    if trimmed.chars().all(|ch| ch.is_ascii_digit()) {
        return Some(trimmed.to_owned());
    }
    let marker = "/properties/";
    let start = trimmed.find(marker)? + marker.len();
    let digits = trimmed[start..]
        .chars()
        .take_while(|ch| ch.is_ascii_digit())
        .collect::<String>();
    (!digits.is_empty()).then_some(digits)
}

fn init_schema(connection: &Connection) -> Result<()> {
    connection.execute_batch(SCHEMA)?;
    connection.pragma_update(None, "user_version", INTELLIGENCE_SCHEMA_VERSION)?;
    validate_schema(connection)
}

fn ensure_additive_schema(connection: &Connection) -> Result<()> {
    connection.execute_batch(CORRECTIONS_SCHEMA)?;
    Ok(())
}

fn validate_schema(connection: &Connection) -> Result<()> {
    let version = read_user_version(connection)?;
    if version != INTELLIGENCE_SCHEMA_VERSION {
        return Err(schema_mismatch(version));
    }
    validate_required_columns(
        connection,
        "assessments",
        &["entity_id", "assessment_json", "saved_at"],
    )?;
    validate_required_columns(
        connection,
        "evidence_bundles",
        &["entity_id", "rightmove_id", "bundle_json", "generated_at"],
    )?;
    validate_required_columns(
        connection,
        "source_snapshots",
        &["id", "entity_id", "source", "source_key", "content_hash"],
    )?;
    Ok(())
}

fn read_user_version(connection: &Connection) -> Result<i32> {
    connection
        .pragma_query_value(None, "user_version", |row| row.get::<_, i32>(0))
        .map_err(Into::into)
}

fn database_has_tables(connection: &Connection) -> Result<bool> {
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

fn table_exists(connection: &Connection, table: &str) -> Result<bool> {
    let count = connection.query_row(
        "SELECT COUNT(*)
         FROM sqlite_master
         WHERE type = 'table'
           AND name = ?1",
        params![table],
        |row| row.get::<_, i64>(0),
    )?;
    Ok(count > 0)
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

fn schema_mismatch(version: i32) -> LetError {
    LetError::new(
        ErrorCode::SchemaMismatch,
        format!(
            "intelligence database schema version {version} does not match expected version {INTELLIGENCE_SCHEMA_VERSION}"
        ),
        "move the incompatible database aside and run `let inspect <rightmove-id>` to recreate it",
    )
}

fn schema_shape_mismatch(message: String) -> LetError {
    LetError::new(
        ErrorCode::SchemaMismatch,
        message,
        "move the incompatible database aside and run `let inspect <rightmove-id>` to recreate it",
    )
}

fn serialize_json<T: Serialize>(value: &T, label: &str) -> Result<String> {
    serde_json::to_string(value).map_err(|error| {
        LetError::new(
            ErrorCode::Internal,
            format!("failed to serialize {label}: {error}"),
            "report this bug",
        )
    })
}

fn deserialize_json<T: DeserializeOwned>(raw: &str, label: &str) -> Result<T> {
    serde_json::from_str(raw).map_err(|error| {
        LetError::new(
            ErrorCode::Parse,
            format!("failed to parse stored {label}: {error}"),
            "rebuild the evidence bundle with `let inspect <id> --refresh all`",
        )
    })
}

fn count_rows(connection: &Connection, table: &str) -> Result<i64> {
    let sql = format!("SELECT COUNT(*) FROM {table}");
    connection
        .query_row(&sql, [], |row| row.get::<_, i64>(0))
        .map_err(Into::into)
}

fn correction_from_row(row: &rusqlite::Row<'_>) -> rusqlite::Result<CorrectionRecord> {
    let kind_raw = row.get::<_, String>(2)?;
    let kind = match kind_raw.as_str() {
        "address" => CorrectionKind::Address,
        "epc" => CorrectionKind::Epc,
        "media" => CorrectionKind::Media,
        _ => {
            return Err(rusqlite::Error::FromSqlConversionFailure(
                2,
                rusqlite::types::Type::Text,
                format!("unknown correction kind `{kind_raw}`").into(),
            ));
        }
    };
    let payload_raw = row.get::<_, String>(3)?;
    let affected_sections_raw = row.get::<_, String>(6)?;
    let payload = serde_json::from_str(&payload_raw).map_err(|error| {
        rusqlite::Error::FromSqlConversionFailure(3, rusqlite::types::Type::Text, Box::new(error))
    })?;
    let affected_sections = serde_json::from_str(&affected_sections_raw).map_err(|error| {
        rusqlite::Error::FromSqlConversionFailure(6, rusqlite::types::Type::Text, Box::new(error))
    })?;
    Ok(CorrectionRecord {
        id: row.get(0)?,
        entity_id: row.get(1)?,
        kind,
        payload,
        note: row.get(4)?,
        active: row.get::<_, i64>(5)? != 0,
        affected_sections,
        created_at: row.get(7)?,
        cleared_at: row.get(8)?,
    })
}

fn stable_row_id(parts: &[&str]) -> String {
    use sha2::{Digest, Sha256};

    let mut hasher = Sha256::new();
    for part in parts {
        hasher.update(part.as_bytes());
        hasher.update([0]);
    }
    format!("{:x}", hasher.finalize())
}

const CORRECTIONS_SCHEMA: &str = r#"
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
"#;

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
"#;

#[cfg(test)]
mod tests {
    use rusqlite::Connection;
    use tempfile::TempDir;

    use super::{
        INTELLIGENCE_SCHEMA_VERSION, IntelligenceDb, extract_rightmove_id, normalize_entity_id,
    };

    #[test]
    fn rightmove_id_extraction_accepts_id_and_url() {
        assert_eq!(
            extract_rightmove_id("170448131").as_deref(),
            Some("170448131")
        );
        assert_eq!(
            extract_rightmove_id(
                "https://www.rightmove.co.uk/properties/170448131#/?channel=RES_LET"
            )
            .as_deref(),
            Some("170448131")
        );
        assert_eq!(
            normalize_entity_id("rightmove:170448131"),
            "rightmove:170448131"
        );
    }

    #[test]
    fn repository_initializes_schema() {
        let temp = TempDir::new().expect("temp dir");
        let db = IntelligenceDb::open(temp.path().join("let.db"));
        assert!(db.is_ok());
    }

    #[test]
    fn repository_rejects_existing_wrong_schema_version_without_mutating_it() {
        let temp = TempDir::new().expect("temp dir");
        let path = temp.path().join("let.db");
        {
            let connection = Connection::open(&path).expect("open db");
            connection
                .pragma_update(None, "user_version", 2)
                .expect("set user_version");
            connection
                .execute("CREATE TABLE legacy (id TEXT PRIMARY KEY)", [])
                .expect("create legacy table");
        }

        let error = match IntelligenceDb::open(&path) {
            Ok(_) => panic!("legacy db should be rejected"),
            Err(error) => error,
        };
        assert_eq!(error.code.as_str(), "SCHEMA_MISMATCH");
        let version = Connection::open(&path)
            .expect("reopen db")
            .pragma_query_value(None, "user_version", |row| row.get::<_, i32>(0))
            .expect("read user_version");
        assert_eq!(version, 2);
    }

    #[test]
    fn repository_rejects_mixed_legacy_table_shape() {
        let temp = TempDir::new().expect("temp dir");
        let path = temp.path().join("let.db");
        {
            let connection = Connection::open(&path).expect("open db");
            connection
                .pragma_update(None, "user_version", INTELLIGENCE_SCHEMA_VERSION)
                .expect("set user_version");
            connection
                .execute(
                    "CREATE TABLE assessments (
                        listing_id TEXT PRIMARY KEY,
                        recommendation TEXT NOT NULL
                    )",
                    [],
                )
                .expect("create legacy assessments table");
            connection
                .execute(
                    "CREATE TABLE evidence_bundles (
                        entity_id TEXT PRIMARY KEY,
                        rightmove_id TEXT NOT NULL,
                        bundle_json TEXT NOT NULL,
                        generated_at TEXT NOT NULL
                    )",
                    [],
                )
                .expect("create evidence table");
            connection
                .execute(
                    "CREATE TABLE source_snapshots (
                        id TEXT PRIMARY KEY,
                        entity_id TEXT NOT NULL,
                        source TEXT NOT NULL,
                        source_key TEXT NOT NULL,
                        content_hash TEXT NOT NULL
                    )",
                    [],
                )
                .expect("create source table");
        }

        let error = match IntelligenceDb::open(&path) {
            Ok(_) => panic!("mixed db should be rejected"),
            Err(error) => error,
        };
        assert_eq!(error.code.as_str(), "SCHEMA_MISMATCH");
        assert!(
            error.message.contains("assessments"),
            "unexpected message: {}",
            error.message
        );
    }
}
