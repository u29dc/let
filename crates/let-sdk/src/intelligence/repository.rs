#![forbid(unsafe_code)]

use std::fs;
use std::path::Path;
use std::time::Duration;

use rusqlite::{Connection, OptionalExtension, params};
use serde::Serialize;
use serde::de::DeserializeOwned;
use serde_json::Value;

use crate::errors::{ErrorCode, LetError, Result};
use crate::intelligence::types::{
    AssessmentRecord, CorrectionKind, CorrectionRecord, EvidenceBundle, ListingListFilters,
    MediaItemEvidence, StoredAssessmentSummary, StoredListingSummary,
};
use crate::utils::time::now_iso;

const INTELLIGENCE_SCHEMA_VERSION: i32 = 1;
const SQLITE_BUSY_TIMEOUT: Duration = Duration::from_millis(5_000);

pub struct IntelligenceDb {
    connection: Connection,
}

#[derive(Debug, Clone)]
struct MediaAssetRow {
    kind: String,
    remote_url: String,
    local_path: Option<String>,
    width: Option<u32>,
    height: Option<u32>,
    content_hash: Option<String>,
    status: String,
}

impl From<&MediaItemEvidence> for MediaAssetRow {
    fn from(item: &MediaItemEvidence) -> Self {
        Self {
            kind: item.kind.clone(),
            remote_url: item.remote_url.clone(),
            local_path: item.local_path.clone(),
            width: item.width,
            height: item.height,
            content_hash: item.content_hash.clone(),
            status: item.status.clone(),
        }
    }
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
        bundle.refresh_derived();
        Ok(bundle)
    }

    pub fn save_bundle(&mut self, bundle: &EvidenceBundle) -> Result<()> {
        let mut bundle = bundle.clone();
        bundle.refresh_derived();
        let bundle = &bundle;
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
        let contact_sheet = bundle
            .media
            .contact_sheet
            .as_ref()
            .map(|sheet| MediaAssetRow {
                kind: "contactSheet".to_owned(),
                remote_url: format!("local://contact-sheet/{}", bundle.entity_id),
                local_path: sheet.local_path.clone(),
                width: sheet.width,
                height: sheet.height,
                content_hash: sheet.content_hash.clone(),
                status: sheet.status.clone(),
            });
        for item in bundle
            .media
            .photos
            .iter()
            .chain(bundle.media.floorplans.iter())
            .chain(bundle.media.epc_graphs.iter())
            .chain(bundle.media.maps.iter())
            .map(MediaAssetRow::from)
            .chain(contact_sheet.iter().cloned())
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
        let record = AssessmentRecord::new(normalized, assessment, now_iso());
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
            Ok(AssessmentRecord::new(
                normalize_entity_id(entity_id),
                deserialize_json(&assessment_json, "assessment")?,
                saved_at,
            ))
        })
        .transpose()
    }

    pub fn list_evidence_summaries(
        &self,
        filters: &ListingListFilters,
    ) -> Result<Vec<StoredListingSummary>> {
        let mut statement = self.connection.prepare(
            "SELECT b.bundle_json, a.assessment_json, a.saved_at, e.updated_at
             FROM evidence_bundles b
             LEFT JOIN assessments a ON a.entity_id = b.entity_id
             LEFT JOIN entities e ON e.id = b.entity_id
             ORDER BY b.generated_at DESC",
        )?;
        let rows = statement.query_map([], |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, Option<String>>(1)?,
                row.get::<_, Option<String>>(2)?,
                row.get::<_, Option<String>>(3)?,
            ))
        })?;

        let mut summaries = Vec::new();
        for row in rows {
            let (bundle_json, assessment_json, assessment_saved_at, updated_at) = row?;
            let bundle = self.deserialize_bundle(&bundle_json)?;
            let assessment =
                joined_assessment(&bundle.entity_id, assessment_json, assessment_saved_at)?
                    .or_else(|| bundle.assessment.clone());
            let summary = summary_from_bundle(&bundle, assessment.as_ref(), updated_at);
            if matches_filters(
                &summary,
                assessment.as_ref().map(|record| &record.assessment),
                filters,
            ) {
                summaries.push(summary);
            }
        }
        Ok(summaries)
    }

    pub fn list_assessment_summaries(
        &self,
        filters: &ListingListFilters,
    ) -> Result<Vec<StoredAssessmentSummary>> {
        let mut statement = self.connection.prepare(
            "SELECT a.entity_id, a.assessment_json, a.saved_at, b.bundle_json, e.updated_at
             FROM assessments a
             LEFT JOIN evidence_bundles b ON b.entity_id = a.entity_id
             LEFT JOIN entities e ON e.id = a.entity_id
             ORDER BY a.saved_at DESC",
        )?;
        let rows = statement.query_map([], |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, String>(2)?,
                row.get::<_, Option<String>>(3)?,
                row.get::<_, Option<String>>(4)?,
            ))
        })?;

        let mut summaries = Vec::new();
        for row in rows {
            let (entity_id, assessment_json, saved_at, bundle_json, updated_at) = row?;
            let assessment = AssessmentRecord::new(
                entity_id.clone(),
                deserialize_json(&assessment_json, "assessment")?,
                saved_at,
            );
            let bundle = bundle_json
                .as_deref()
                .map(|raw| self.deserialize_bundle(raw))
                .transpose()?;
            let listing = match bundle.as_ref() {
                Some(bundle) => summary_from_bundle(bundle, Some(&assessment), updated_at),
                None => summary_from_assessment(&assessment, updated_at),
            };
            if matches_filters(&listing, Some(&assessment.assessment), filters) {
                let normalized = assessment.normalized_assessment.clone();
                summaries.push(StoredAssessmentSummary {
                    listing,
                    summary: normalized.summary.clone(),
                    positives: normalized.positives.clone(),
                    risks: normalized.risks.clone(),
                    next_actions: normalized.next_actions.clone(),
                    tradeoffs: normalized.tradeoffs.clone(),
                    area_notes: normalized.area_notes.clone(),
                    commute_notes: normalized.commute_notes.clone(),
                    family_fit: normalized.family_fit.clone(),
                    evidence_gaps: normalized.evidence_gaps.clone(),
                    source: normalized.source.clone(),
                    normalized_warnings: normalized.warnings.clone(),
                    normalized_assessment: normalized,
                    assessment: assessment.assessment,
                });
            }
        }
        Ok(summaries)
    }
}

fn joined_assessment(
    entity_id: &str,
    assessment_json: Option<String>,
    saved_at: Option<String>,
) -> Result<Option<AssessmentRecord>> {
    let Some(assessment_json) = assessment_json else {
        return Ok(None);
    };
    let Some(saved_at) = saved_at else {
        return Ok(None);
    };
    Ok(Some(AssessmentRecord::new(
        entity_id.to_owned(),
        deserialize_json(&assessment_json, "assessment")?,
        saved_at,
    )))
}

fn summary_from_bundle(
    bundle: &EvidenceBundle,
    assessment: Option<&AssessmentRecord>,
    updated_at: Option<String>,
) -> StoredListingSummary {
    let address = bundle.rightmove.address.clone().or_else(|| {
        bundle
            .address
            .selected
            .as_ref()
            .map(|selected| selected.label.clone())
    });
    let postcode = bundle
        .rightmove
        .postcode
        .clone()
        .or_else(|| {
            bundle
                .address
                .selected
                .as_ref()
                .and_then(|selected| selected.postcode.clone())
        })
        .or_else(|| {
            bundle.broadband.as_ref().and_then(|broadband| {
                broadband
                    .postcode_display
                    .clone()
                    .or_else(|| Some(broadband.postcode.clone()))
            })
        });
    let price = bundle.rightmove.display_price.clone().or_else(|| {
        bundle
            .rightmove
            .price_pcm
            .map(|price| format!("{price} pcm"))
    });
    let assessment_value = assessment.map(|record| &record.assessment);
    let normalized_assessment = assessment.map(|record| &record.normalized_assessment);
    let area =
        assessment_text(assessment_value, &["area", "locationArea", "region"]).or_else(|| {
            bundle
                .broadband
                .as_ref()
                .and_then(|broadband| broadband.outward.clone().or_else(|| broadband.area.clone()))
        });

    StoredListingSummary {
        id: external_id_from_entity(&bundle.entity_id, Some(&bundle.rightmove_id)),
        entity_id: bundle.entity_id.clone(),
        url: Some(bundle.url.clone()),
        address,
        postcode,
        area,
        price,
        price_pcm: bundle.rightmove.price_pcm,
        recommendation: normalized_assessment
            .and_then(|assessment| assessment.recommendation.clone())
            .or_else(|| assessment_text(assessment_value, &["recommendation"])),
        confidence: normalized_assessment
            .and_then(|assessment| assessment.confidence.clone())
            .or_else(|| assessment_text(assessment_value, &["confidence"])),
        saved_at: assessment.map(|record| record.saved_at.clone()),
        inspected_at: Some(bundle.generated_at.clone()),
        updated_at,
    }
}

fn summary_from_assessment(
    assessment: &AssessmentRecord,
    updated_at: Option<String>,
) -> StoredListingSummary {
    StoredListingSummary {
        id: external_id_from_entity(&assessment.entity_id, None),
        entity_id: assessment.entity_id.clone(),
        url: None,
        address: None,
        postcode: None,
        area: assessment_text(
            Some(&assessment.assessment),
            &["area", "locationArea", "region"],
        ),
        price: None,
        price_pcm: None,
        recommendation: assessment
            .normalized_assessment
            .recommendation
            .clone()
            .or_else(|| assessment_text(Some(&assessment.assessment), &["recommendation"])),
        confidence: assessment
            .normalized_assessment
            .confidence
            .clone()
            .or_else(|| assessment_text(Some(&assessment.assessment), &["confidence"])),
        saved_at: Some(assessment.saved_at.clone()),
        inspected_at: None,
        updated_at,
    }
}

fn external_id_from_entity(entity_id: &str, rightmove_id: Option<&str>) -> String {
    rightmove_id
        .filter(|value| !value.trim().is_empty())
        .map(ToOwned::to_owned)
        .or_else(|| entity_id.strip_prefix("rightmove:").map(ToOwned::to_owned))
        .unwrap_or_else(|| entity_id.to_owned())
}

fn assessment_text(assessment: Option<&Value>, keys: &[&str]) -> Option<String> {
    let assessment = assessment?;
    for key in keys {
        if let Some(value) = assessment.get(*key).and_then(value_to_filter_text) {
            return Some(value);
        }
    }
    None
}

fn value_to_filter_text(value: &Value) -> Option<String> {
    match value {
        Value::String(text) => non_empty_text(text),
        Value::Number(number) => Some(number.to_string()),
        Value::Bool(flag) => Some(flag.to_string()),
        _ => None,
    }
}

fn non_empty_text(text: &str) -> Option<String> {
    let trimmed = text.trim();
    (!trimmed.is_empty()).then(|| trimmed.to_owned())
}

fn matches_filters(
    summary: &StoredListingSummary,
    assessment: Option<&Value>,
    filters: &ListingListFilters,
) -> bool {
    matches_recommendation(summary, filters)
        && matches_confidence(summary, filters)
        && matches_max_price(summary, filters)
        && matches_postcode_prefix(summary, filters)
        && matches_area(summary, assessment, filters)
}

fn matches_recommendation(summary: &StoredListingSummary, filters: &ListingListFilters) -> bool {
    let Some(expected) = filters.recommendation.as_deref().and_then(non_empty_text) else {
        return true;
    };
    summary
        .recommendation
        .as_deref()
        .is_some_and(|actual| actual.eq_ignore_ascii_case(&expected))
}

fn matches_confidence(summary: &StoredListingSummary, filters: &ListingListFilters) -> bool {
    let Some(expected) = filters.confidence.as_deref().and_then(non_empty_text) else {
        return true;
    };
    summary
        .confidence
        .as_deref()
        .is_some_and(|actual| actual.eq_ignore_ascii_case(&expected))
}

fn matches_max_price(summary: &StoredListingSummary, filters: &ListingListFilters) -> bool {
    let Some(max_price) = filters.max_price else {
        return true;
    };
    summary.price_pcm.is_some_and(|price| price <= max_price)
}

fn matches_postcode_prefix(summary: &StoredListingSummary, filters: &ListingListFilters) -> bool {
    let Some(prefix) = filters
        .postcode_prefix
        .as_deref()
        .map(normalize_postcode)
        .filter(|value| !value.is_empty())
    else {
        return true;
    };
    summary
        .postcode
        .as_deref()
        .map(normalize_postcode)
        .is_some_and(|postcode| postcode.starts_with(&prefix))
}

fn matches_area(
    summary: &StoredListingSummary,
    assessment: Option<&Value>,
    filters: &ListingListFilters,
) -> bool {
    let Some(expected) = filters.area.as_deref().and_then(non_empty_text) else {
        return true;
    };
    let expected = expected.to_ascii_lowercase();
    [
        summary.area.as_deref(),
        summary.address.as_deref(),
        summary.postcode.as_deref(),
        assessment_text(assessment, &["area", "locationArea", "region"]).as_deref(),
    ]
    .into_iter()
    .flatten()
    .any(|value| value.to_ascii_lowercase().contains(&expected))
}

fn normalize_postcode(value: &str) -> String {
    value
        .chars()
        .filter(|ch| !ch.is_whitespace())
        .flat_map(char::to_uppercase)
        .collect()
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
    use serde_json::json;
    use tempfile::TempDir;

    use crate::intelligence::types::{
        AddressCandidateEvidence, AddressEvidence, BroadbandEvidence, ConfidenceLevel,
        ContactSheetEvidence, DescriptionEvidence, EvidenceBundle, InspectDepth,
        ListingListFilters, MediaEvidence, RefreshPolicy, RightmoveEvidence, SectionStatus,
    };

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

    #[test]
    fn repository_lists_evidence_and_assessments_with_filters() {
        let temp = TempDir::new().expect("temp dir");
        let path = temp.path().join("let.db");
        let mut db = IntelligenceDb::open(&path).expect("open db");
        db.save_bundle(&test_bundle("170448131", "M1 1AA", 1250))
            .expect("save matching bundle");
        db.save_assessment(
            "170448131",
            json!({
                "recommendation": "view",
                "confidence": "high",
                "area": "Manchester"
            }),
        )
        .expect("save matching assessment");
        db.save_bundle(&test_bundle("170448132", "M2 2BB", 1750))
            .expect("save filtered bundle");
        db.save_assessment(
            "170448132",
            json!({
                "recommendation": "skip",
                "confidence": "medium",
                "area": "Manchester"
            }),
        )
        .expect("save filtered assessment");

        let filters = ListingListFilters {
            recommendation: Some("view".to_owned()),
            confidence: Some("high".to_owned()),
            area: Some("manchester".to_owned()),
            max_price: Some(1300),
            postcode_prefix: Some("M1".to_owned()),
        };

        let evidence = db.list_evidence_summaries(&filters).expect("list evidence");
        assert_eq!(evidence.len(), 1);
        assert_eq!(evidence[0].id, "170448131");
        assert_eq!(evidence[0].entity_id, "rightmove:170448131");
        assert_eq!(evidence[0].postcode.as_deref(), Some("M1 1AA"));
        assert_eq!(evidence[0].price_pcm, Some(1250));
        assert_eq!(evidence[0].recommendation.as_deref(), Some("view"));
        assert_eq!(evidence[0].confidence.as_deref(), Some("high"));
        assert!(evidence[0].saved_at.is_some());
        assert_eq!(
            evidence[0].inspected_at.as_deref(),
            Some("2026-06-18T00:00:00.000Z")
        );

        let assessments = db
            .list_assessment_summaries(&filters)
            .expect("list assessments");
        assert_eq!(assessments.len(), 1);
        assert_eq!(assessments[0].listing.id, "170448131");
        assert_eq!(assessments[0].assessment["recommendation"], "view");
    }

    #[test]
    fn repository_persists_contact_sheet_media_asset() {
        let temp = TempDir::new().expect("temp dir");
        let path = temp.path().join("let.db");
        let mut db = IntelligenceDb::open(&path).expect("open db");
        let mut bundle = test_bundle("170448131", "M1 1AA", 1250);
        bundle.media.contact_sheet = Some(ContactSheetEvidence {
            status: "generated".to_owned(),
            photo_count: 3,
            local_path: Some("/tmp/contact-sheet.jpg".to_owned()),
            generated_at: Some("2026-06-20T00:00:00Z".to_owned()),
            width: Some(932),
            height: Some(241),
            content_hash: Some("hash".to_owned()),
        });

        db.save_bundle(&bundle).expect("save bundle");

        let row = db
            .connection
            .query_row(
                "SELECT remote_url, local_path, status FROM media_assets WHERE kind = 'contactSheet'",
                [],
                |row| {
                    Ok((
                        row.get::<_, String>(0)?,
                        row.get::<_, Option<String>>(1)?,
                        row.get::<_, String>(2)?,
                    ))
                },
            )
            .expect("contact sheet row");
        assert_eq!(row.0, "local://contact-sheet/rightmove:170448131");
        assert_eq!(row.1.as_deref(), Some("/tmp/contact-sheet.jpg"));
        assert_eq!(row.2, "generated");
    }

    fn test_bundle(rightmove_id: &str, postcode: &str, price_pcm: i64) -> EvidenceBundle {
        let entity_id = format!("rightmove:{rightmove_id}");
        let url = format!("https://www.rightmove.co.uk/properties/{rightmove_id}");
        EvidenceBundle {
            entity_id: entity_id.clone(),
            rightmove_id: rightmove_id.to_owned(),
            url: url.clone(),
            generated_at: "2026-06-18T00:00:00.000Z".to_owned(),
            depth: InspectDepth::Standard,
            refresh: RefreshPolicy::Stale,
            sections: Default::default(),
            source_snapshots: Vec::new(),
            rightmove: RightmoveEvidence {
                rightmove_id: rightmove_id.to_owned(),
                url,
                page_status: "active".to_owned(),
                fetched_at: "2026-06-18T00:00:00.000Z".to_owned(),
                content_hash: format!("hash-{rightmove_id}"),
                title: Some("Two bedroom flat".to_owned()),
                address: Some("1 Example Street, Manchester".to_owned()),
                postcode: Some(postcode.to_owned()),
                display_price: Some(format!("{price_pcm} pcm")),
                price_pcm: Some(price_pcm),
                bedrooms: Some(2),
                bathrooms: Some(1),
                property_type: Some("Flat".to_owned()),
                agent_name: None,
                agent_phone: None,
                latitude: None,
                longitude: None,
                pin_type: None,
                listed_date: None,
                available_date: None,
                deposit: None,
                description: DescriptionEvidence {
                    raw_html: String::new(),
                    text: String::new(),
                    key_features: Vec::new(),
                    normalized_text: String::new(),
                },
                media: Vec::new(),
            },
            address: AddressEvidence {
                candidates: vec![AddressCandidateEvidence {
                    source: "rightmove".to_owned(),
                    label: "1 Example Street, Manchester".to_owned(),
                    postcode: Some(postcode.to_owned()),
                    latitude: None,
                    longitude: None,
                    confidence: ConfidenceLevel::Probable,
                    raw: None,
                }],
                selected: None,
                status: SectionStatus::Ok,
                confidence: ConfidenceLevel::Probable,
                warnings: Vec::new(),
            },
            facts: Vec::new(),
            broadband: Some(BroadbandEvidence {
                postcode: postcode.replace(' ', ""),
                postcode_display: Some(postcode.to_owned()),
                outward: postcode.split_whitespace().next().map(ToOwned::to_owned),
                area: Some("M".to_owned()),
                gigabit_availability: None,
                pct_over_300mbps: None,
                ufbb_availability: None,
                sfbb_availability: None,
            }),
            epc: None,
            claims: Vec::new(),
            verifications: Vec::new(),
            media: MediaEvidence::default(),
            assessment: None,
            corrections: Vec::new(),
            next_actions: Vec::new(),
            flags: Vec::new(),
        }
    }
}
