#![forbid(unsafe_code)]

use std::path::Path;

use rusqlite::{Connection, OpenFlags, OptionalExtension, params};
use serde::Serialize;

use crate::errors::{ErrorCode, LetError, Result};
use crate::pipeline::uprn::UprnDistanceCandidate;
use crate::schema::listing::Listing;
use crate::utils::text::normalize_postcode;

const SOURCE_POSTCODES: &str = "postcodes";
const SOURCE_BROADBAND: &str = "broadband";
const SOURCE_DEPRIVATION: &str = "deprivation";
const SOURCE_CENSUS: &str = "census";
const SOURCE_POPULATION: &str = "population";
const SOURCE_INCOME: &str = "income";
const SOURCE_FLOOD: &str = "flood";
const SOURCE_CRIME: &str = "crime";
const SOURCE_UPRN: &str = "uprn";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EnrichmentMode {
    ReplaceFromSources,
    FillMissingFromSources,
}

#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct EnrichmentJoinKeys {
    pub postcode: Option<String>,
    pub lsoa_code: Option<String>,
    pub msoa_code: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ListingEnrichmentReport {
    pub listing_id: String,
    pub join_keys: EnrichmentJoinKeys,
    pub applied_fields: Vec<String>,
    pub missing_categories: Vec<String>,
    pub unavailable_sources: Vec<String>,
}

pub struct SourceEnricher {
    postcodes: Option<Connection>,
    broadband: Option<Connection>,
    deprivation: Option<Connection>,
    census: Option<Connection>,
    population: Option<Connection>,
    income: Option<Connection>,
    flood: Option<Connection>,
    crime: Option<Connection>,
    uprn: Option<Connection>,
    unavailable_sources: Vec<String>,
}

#[derive(Debug, Clone)]
struct PostcodeLookup {
    lsoa_code: Option<String>,
    lsoa_name: Option<String>,
    msoa_code: Option<String>,
    msoa_name: Option<String>,
}

#[derive(Debug, Clone)]
pub struct PostcodeCoordinates {
    pub lat: f64,
    pub lng: f64,
}

#[derive(Debug, Clone)]
struct FloodLookup {
    level: Option<String>,
    source: Option<String>,
}

#[derive(Debug, Clone)]
struct CrimeLookup {
    total: Option<i64>,
    violent: Option<i64>,
    burglary: Option<i64>,
    robbery: Option<i64>,
    month_end: Option<String>,
}

#[derive(Debug, Clone)]
struct ImdLookup {
    rank: Option<i64>,
    decile: Option<i64>,
    score: Option<f64>,
}

impl SourceEnricher {
    pub fn open(sources_dir: &Path) -> Result<Self> {
        let mut unavailable_sources = Vec::new();
        let postcodes =
            open_optional_source_db(sources_dir, SOURCE_POSTCODES, &mut unavailable_sources)?;
        let broadband =
            open_optional_source_db(sources_dir, SOURCE_BROADBAND, &mut unavailable_sources)?;
        let deprivation =
            open_optional_source_db(sources_dir, SOURCE_DEPRIVATION, &mut unavailable_sources)?;
        let census = open_optional_source_db(sources_dir, SOURCE_CENSUS, &mut unavailable_sources)?;
        let population =
            open_optional_source_db(sources_dir, SOURCE_POPULATION, &mut unavailable_sources)?;
        let income = open_optional_source_db(sources_dir, SOURCE_INCOME, &mut unavailable_sources)?;
        let flood = open_optional_source_db(sources_dir, SOURCE_FLOOD, &mut unavailable_sources)?;
        let crime = open_optional_source_db(sources_dir, SOURCE_CRIME, &mut unavailable_sources)?;
        let uprn = open_optional_source_db(sources_dir, SOURCE_UPRN, &mut unavailable_sources)?;
        unavailable_sources.sort();

        Ok(Self {
            postcodes,
            broadband,
            deprivation,
            census,
            population,
            income,
            flood,
            crime,
            uprn,
            unavailable_sources,
        })
    }

    pub fn enrich_listings(
        &self,
        listings: &mut [Listing],
        mode: EnrichmentMode,
    ) -> Result<Vec<ListingEnrichmentReport>> {
        listings
            .iter_mut()
            .map(|listing| self.enrich_listing(listing, mode))
            .collect()
    }

    pub fn enrich_listing(
        &self,
        listing: &mut Listing,
        mode: EnrichmentMode,
    ) -> Result<ListingEnrichmentReport> {
        let before = listing.clone();
        let postcode_key = normalize_non_empty_postcode(&listing.postcode);

        let postcode_lookup = if let (Some(connection), Some(postcode)) =
            (self.postcodes.as_ref(), postcode_key.as_deref())
        {
            query_postcode_lookup(connection, postcode)?
        } else {
            None
        };

        if self.postcodes.is_some() {
            apply_option(
                &mut listing.area.lsoa.code,
                postcode_lookup
                    .as_ref()
                    .and_then(|item| item.lsoa_code.clone()),
                mode,
            );
            apply_option(
                &mut listing.area.lsoa.name,
                postcode_lookup
                    .as_ref()
                    .and_then(|item| item.lsoa_name.clone()),
                mode,
            );
            apply_option(
                &mut listing.area.msoa.code,
                postcode_lookup
                    .as_ref()
                    .and_then(|item| item.msoa_code.clone()),
                mode,
            );
            apply_option(
                &mut listing.area.msoa.name,
                postcode_lookup
                    .as_ref()
                    .and_then(|item| item.msoa_name.clone()),
                mode,
            );
        }

        let lsoa_code = normalize_area_code(
            postcode_lookup
                .as_ref()
                .and_then(|item| item.lsoa_code.as_deref())
                .or(listing.area.lsoa.code.as_deref()),
        );
        let msoa_code = normalize_area_code(
            postcode_lookup
                .as_ref()
                .and_then(|item| item.msoa_code.as_deref())
                .or(listing.area.msoa.code.as_deref()),
        );

        if let Some(connection) = self.broadband.as_ref() {
            let gigabit = if let Some(postcode) = postcode_key.as_deref() {
                query_broadband_gigabit(connection, postcode)?
            } else {
                None
            };
            apply_option(&mut listing.gigabit_availability, gigabit, mode);
        }

        if let Some(connection) = self.flood.as_ref() {
            let flood = if let Some(postcode) = postcode_key.as_deref() {
                query_flood_lookup(connection, postcode)?
            } else {
                None
            };
            apply_option(
                &mut listing.area.flood_risk.level,
                flood.as_ref().and_then(|item| item.level.clone()),
                mode,
            );
            apply_option(
                &mut listing.area.flood_risk.source,
                flood.and_then(|item| item.source),
                mode,
            );
        }

        if let Some(connection) = self.deprivation.as_ref() {
            let imd = if let Some(code) = lsoa_code.as_deref() {
                query_imd_lookup(connection, code)?
            } else {
                None
            };
            apply_option(
                &mut listing.area.imd.rank,
                imd.as_ref().and_then(|item| item.rank),
                mode,
            );
            apply_option(
                &mut listing.area.imd.decile,
                imd.as_ref().and_then(|item| item.decile),
                mode,
            );
            apply_option(
                &mut listing.area.imd.score,
                imd.as_ref().and_then(|item| item.score),
                mode,
            );
        }

        if let Some(connection) = self.census.as_ref() {
            let social_housing_pct = if let Some(code) = lsoa_code.as_deref() {
                query_social_housing_pct(connection, code)?
            } else {
                None
            };
            apply_option(
                &mut listing.area.social_housing_pct,
                social_housing_pct,
                mode,
            );
        }

        if let Some(connection) = self.population.as_ref() {
            let population = if let Some(code) = lsoa_code.as_deref() {
                query_population(connection, code)?
            } else {
                None
            };
            apply_option(&mut listing.area.population, population, mode);
        }

        if let Some(connection) = self.income.as_ref() {
            let income = if let Some(code) = msoa_code.as_deref() {
                query_income(connection, code)?
            } else {
                None
            };
            apply_option(
                &mut listing.area.income.bhc,
                income.as_ref().and_then(|item| item.0),
                mode,
            );
            apply_option(
                &mut listing.area.income.ahc,
                income.and_then(|item| item.1),
                mode,
            );
        }

        if let Some(connection) = self.crime.as_ref() {
            let crime = if let Some(code) = lsoa_code.as_deref() {
                query_crime_lookup(connection, code)?
            } else {
                None
            };
            apply_option(
                &mut listing.area.crime.count_12m,
                crime.as_ref().and_then(|item| item.total),
                mode,
            );
            apply_option(
                &mut listing.area.crime.violent_12m,
                crime.as_ref().and_then(|item| item.violent),
                mode,
            );
            apply_option(
                &mut listing.area.crime.burglary_12m,
                crime.as_ref().and_then(|item| item.burglary),
                mode,
            );
            apply_option(
                &mut listing.area.crime.robbery_12m,
                crime.as_ref().and_then(|item| item.robbery),
                mode,
            );
            apply_option(
                &mut listing.area.crime.updated_at,
                crime.as_ref().and_then(|item| item.month_end.clone()),
                mode,
            );

            let derived_rate = match (listing.area.crime.count_12m, listing.area.population) {
                (Some(total), Some(population)) if population > 0 => {
                    let rate =
                        ((total as f64 / population as f64) * 1_000.0 * 100.0).round() / 100.0;
                    Some(rate)
                }
                _ => None,
            };
            apply_option(&mut listing.area.crime.rate_per_1k, derived_rate, mode);
        }

        let mut missing_categories = Vec::new();
        if self.postcodes.is_some()
            && (listing.area.lsoa.code.is_none() || listing.area.msoa.code.is_none())
        {
            missing_categories.push(SOURCE_POSTCODES.to_owned());
        }
        if self.broadband.is_some() && listing.gigabit_availability.is_none() {
            missing_categories.push(SOURCE_BROADBAND.to_owned());
        }
        if self.flood.is_some()
            && listing.area.flood_risk.level.is_none()
            && listing.area.flood_risk.source.is_none()
        {
            missing_categories.push(SOURCE_FLOOD.to_owned());
        }
        if self.deprivation.is_some()
            && listing.area.imd.rank.is_none()
            && listing.area.imd.decile.is_none()
            && listing.area.imd.score.is_none()
        {
            missing_categories.push(SOURCE_DEPRIVATION.to_owned());
        }
        if self.census.is_some() && listing.area.social_housing_pct.is_none() {
            missing_categories.push(SOURCE_CENSUS.to_owned());
        }
        if self.population.is_some() && listing.area.population.is_none() {
            missing_categories.push(SOURCE_POPULATION.to_owned());
        }
        if self.income.is_some()
            && listing.area.income.bhc.is_none()
            && listing.area.income.ahc.is_none()
        {
            missing_categories.push(SOURCE_INCOME.to_owned());
        }
        if self.crime.is_some()
            && listing.area.crime.count_12m.is_none()
            && listing.area.crime.violent_12m.is_none()
            && listing.area.crime.burglary_12m.is_none()
            && listing.area.crime.robbery_12m.is_none()
        {
            missing_categories.push(SOURCE_CRIME.to_owned());
        }
        missing_categories.sort();

        let join_keys = EnrichmentJoinKeys {
            postcode: postcode_key,
            lsoa_code,
            msoa_code,
        };

        Ok(ListingEnrichmentReport {
            listing_id: listing.id.clone(),
            join_keys,
            applied_fields: changed_fields(&before, listing),
            missing_categories,
            unavailable_sources: self.unavailable_sources.clone(),
        })
    }

    pub fn lookup_postcode_coordinates(
        &self,
        postcode: &str,
    ) -> Result<Option<PostcodeCoordinates>> {
        let Some(connection) = self.postcodes.as_ref() else {
            return Ok(None);
        };
        query_postcode_coordinates(connection, postcode)
    }

    pub fn lookup_uprn_candidates(
        &self,
        lat: f64,
        lng: f64,
        max_distance_m: f64,
        limit: usize,
    ) -> Result<Vec<UprnDistanceCandidate>> {
        let Some(connection) = self.uprn.as_ref() else {
            return Ok(Vec::new());
        };
        query_uprn_candidates(connection, lat, lng, max_distance_m, limit)
    }
}

fn open_optional_source_db(
    sources_dir: &Path,
    source_name: &str,
    unavailable_sources: &mut Vec<String>,
) -> Result<Option<Connection>> {
    let db_path = sources_dir.join(format!("{source_name}.db"));
    if !db_path.exists() {
        unavailable_sources.push(source_name.to_owned());
        return Ok(None);
    }

    let connection = Connection::open_with_flags(&db_path, OpenFlags::SQLITE_OPEN_READ_ONLY)
        .map_err(|error| {
            LetError::new(
                ErrorCode::SchemaMismatch,
                format!(
                    "failed to open source db `{source_name}` at {}: {error}",
                    db_path.display()
                ),
                "run `let build sources all` and retry",
            )
        })?;
    Ok(Some(connection))
}

fn query_postcode_lookup(
    connection: &Connection,
    postcode: &str,
) -> Result<Option<PostcodeLookup>> {
    connection
        .query_row(
            "SELECT lsoa_code, lsoa_name, msoa_code, msoa_name
             FROM postcodes
             WHERE postcode = ?1 OR REPLACE(postcode_display, ' ', '') = ?1
             LIMIT 1",
            params![postcode],
            |row| {
                Ok(PostcodeLookup {
                    lsoa_code: normalize_area_code(row.get::<_, Option<String>>(0)?.as_deref()),
                    lsoa_name: normalize_text(row.get::<_, Option<String>>(1)?),
                    msoa_code: normalize_area_code(row.get::<_, Option<String>>(2)?.as_deref()),
                    msoa_name: normalize_text(row.get::<_, Option<String>>(3)?),
                })
            },
        )
        .optional()
        .map_err(Into::into)
}

fn query_postcode_coordinates(
    connection: &Connection,
    postcode: &str,
) -> Result<Option<PostcodeCoordinates>> {
    connection
        .query_row(
            "SELECT lat, lng FROM postcodes
             WHERE postcode = ?1 OR REPLACE(postcode_display, ' ', '') = ?1
             LIMIT 1",
            params![postcode],
            |row| {
                let lat: Option<f64> = row.get(0)?;
                let lng: Option<f64> = row.get(1)?;
                Ok(match (lat, lng) {
                    (Some(lat), Some(lng)) if !(lat == 0.0 && lng == 0.0) => {
                        Some(PostcodeCoordinates { lat, lng })
                    }
                    _ => None,
                })
            },
        )
        .optional()
        .map(|value| value.flatten())
        .map_err(Into::into)
}

fn query_broadband_gigabit(connection: &Connection, postcode: &str) -> Result<Option<f64>> {
    connection
        .query_row(
            "SELECT gigabit_availability
             FROM postcodes
             WHERE postcode = ?1 OR REPLACE(postcode_display, ' ', '') = ?1
             LIMIT 1",
            params![postcode],
            |row| row.get::<_, Option<f64>>(0),
        )
        .optional()
        .map(|value| value.flatten())
        .map_err(Into::into)
}

fn query_flood_lookup(connection: &Connection, postcode: &str) -> Result<Option<FloodLookup>> {
    connection
        .query_row(
            "SELECT risk, source FROM flood WHERE postcode = ?1 LIMIT 1",
            params![postcode],
            |row| {
                Ok(FloodLookup {
                    level: normalize_text(row.get::<_, Option<String>>(0)?),
                    source: normalize_text(row.get::<_, Option<String>>(1)?),
                })
            },
        )
        .optional()
        .map_err(Into::into)
}

fn query_imd_lookup(connection: &Connection, lsoa_code: &str) -> Result<Option<ImdLookup>> {
    connection
        .query_row(
            "SELECT rank, decile, score FROM imd WHERE lsoa_code = ?1 LIMIT 1",
            params![lsoa_code],
            |row| {
                Ok(ImdLookup {
                    rank: row.get::<_, Option<i64>>(0)?,
                    decile: row.get::<_, Option<i64>>(1)?,
                    score: row.get::<_, Option<f64>>(2)?,
                })
            },
        )
        .optional()
        .map_err(Into::into)
}

fn query_social_housing_pct(connection: &Connection, lsoa_code: &str) -> Result<Option<f64>> {
    connection
        .query_row(
            "SELECT social_housing_pct FROM tenure WHERE lsoa_code = ?1 LIMIT 1",
            params![lsoa_code],
            |row| row.get::<_, Option<f64>>(0),
        )
        .optional()
        .map(|value| value.flatten())
        .map_err(Into::into)
}

fn query_population(connection: &Connection, lsoa_code: &str) -> Result<Option<i64>> {
    connection
        .query_row(
            "SELECT population FROM population WHERE lsoa_code = ?1 LIMIT 1",
            params![lsoa_code],
            |row| row.get::<_, Option<i64>>(0),
        )
        .optional()
        .map(|value| value.flatten())
        .map_err(Into::into)
}

fn query_income(
    connection: &Connection,
    msoa_code: &str,
) -> Result<Option<(Option<f64>, Option<f64>)>> {
    connection
        .query_row(
            "SELECT income_bhc, income_ahc FROM income WHERE msoa_code = ?1 LIMIT 1",
            params![msoa_code],
            |row| Ok((row.get::<_, Option<f64>>(0)?, row.get::<_, Option<f64>>(1)?)),
        )
        .optional()
        .map_err(Into::into)
}

fn query_crime_lookup(connection: &Connection, lsoa_code: &str) -> Result<Option<CrimeLookup>> {
    connection
        .query_row(
            "SELECT total, violent, burglary, robbery, month_end
             FROM crime_12m
             WHERE lsoa_code = ?1
             LIMIT 1",
            params![lsoa_code],
            |row| {
                Ok(CrimeLookup {
                    total: row.get::<_, Option<i64>>(0)?,
                    violent: row.get::<_, Option<i64>>(1)?,
                    burglary: row.get::<_, Option<i64>>(2)?,
                    robbery: row.get::<_, Option<i64>>(3)?,
                    month_end: normalize_text(row.get::<_, Option<String>>(4)?),
                })
            },
        )
        .optional()
        .map_err(Into::into)
}

fn query_uprn_candidates(
    connection: &Connection,
    lat: f64,
    lng: f64,
    max_distance_m: f64,
    limit: usize,
) -> Result<Vec<UprnDistanceCandidate>> {
    let lat_delta = max_distance_m / 111_320.0;
    let cos_lat = lat.to_radians().cos().abs().max(0.1);
    let lng_delta = max_distance_m / (111_320.0 * cos_lat);

    let mut statement = connection.prepare(
        "SELECT uprn, lat, lng
         FROM uprn
         WHERE lat IS NOT NULL
           AND lng IS NOT NULL
           AND lat BETWEEN ?1 AND ?2
           AND lng BETWEEN ?3 AND ?4
         ORDER BY ABS(lat - ?5) + ABS(lng - ?6)
         LIMIT ?7",
    )?;

    let rows = statement.query_map(
        params![
            lat - lat_delta,
            lat + lat_delta,
            lng - lng_delta,
            lng + lng_delta,
            lat,
            lng,
            limit as i64
        ],
        |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, f64>(1)?,
                row.get::<_, f64>(2)?,
            ))
        },
    )?;

    let mut candidates = Vec::new();
    for row in rows {
        let (uprn, candidate_lat, candidate_lng) = row?;
        let distance_m = haversine_distance_m(lat, lng, candidate_lat, candidate_lng);
        if distance_m <= max_distance_m {
            candidates.push(UprnDistanceCandidate { uprn, distance_m });
        }
    }
    candidates.sort_by(|left, right| {
        left.distance_m
            .partial_cmp(&right.distance_m)
            .unwrap_or(std::cmp::Ordering::Equal)
    });
    Ok(candidates)
}

fn haversine_distance_m(left_lat: f64, left_lng: f64, right_lat: f64, right_lng: f64) -> f64 {
    let earth_radius_m = 6_371_000.0;
    let d_lat = (right_lat - left_lat).to_radians();
    let d_lng = (right_lng - left_lng).to_radians();
    let left_lat = left_lat.to_radians();
    let right_lat = right_lat.to_radians();

    let a = (d_lat / 2.0).sin().powi(2)
        + left_lat.cos() * right_lat.cos() * (d_lng / 2.0).sin().powi(2);
    let c = 2.0 * a.sqrt().atan2((1.0 - a).sqrt());
    earth_radius_m * c
}

fn normalize_non_empty_postcode(value: &str) -> Option<String> {
    let normalized = normalize_postcode(value);
    (!normalized.is_empty()).then_some(normalized)
}

fn normalize_area_code(value: Option<&str>) -> Option<String> {
    value
        .map(str::trim)
        .filter(|item| !item.is_empty())
        .map(|item| item.to_ascii_uppercase())
}

fn normalize_text(value: Option<String>) -> Option<String> {
    value
        .map(|item| item.trim().to_owned())
        .filter(|item| !item.is_empty())
}

fn apply_option<T>(target: &mut Option<T>, incoming: Option<T>, mode: EnrichmentMode) {
    match mode {
        EnrichmentMode::ReplaceFromSources => *target = incoming,
        EnrichmentMode::FillMissingFromSources => {
            if target.is_none() {
                *target = incoming;
            }
        }
    }
}

fn changed_fields(before: &Listing, after: &Listing) -> Vec<String> {
    let mut fields = Vec::new();
    if before.area.lsoa.code != after.area.lsoa.code {
        fields.push("lsoaCode".to_owned());
    }
    if before.area.lsoa.name != after.area.lsoa.name {
        fields.push("lsoaName".to_owned());
    }
    if before.area.msoa.code != after.area.msoa.code {
        fields.push("msoaCode".to_owned());
    }
    if before.area.msoa.name != after.area.msoa.name {
        fields.push("msoaName".to_owned());
    }
    if before.gigabit_availability != after.gigabit_availability {
        fields.push("gigabitAvailability".to_owned());
    }
    if before.area.imd.rank != after.area.imd.rank {
        fields.push("imdRank".to_owned());
    }
    if before.area.imd.decile != after.area.imd.decile {
        fields.push("imdDecile".to_owned());
    }
    if before.area.imd.score != after.area.imd.score {
        fields.push("imdScore".to_owned());
    }
    if before.area.income.bhc != after.area.income.bhc {
        fields.push("incomeBhc".to_owned());
    }
    if before.area.income.ahc != after.area.income.ahc {
        fields.push("incomeAhc".to_owned());
    }
    if before.area.social_housing_pct != after.area.social_housing_pct {
        fields.push("socialHousingPct".to_owned());
    }
    if before.area.population != after.area.population {
        fields.push("population".to_owned());
    }
    if before.area.flood_risk.level != after.area.flood_risk.level {
        fields.push("floodRiskLevel".to_owned());
    }
    if before.area.flood_risk.source != after.area.flood_risk.source {
        fields.push("floodRiskSource".to_owned());
    }
    if before.area.crime.count_12m != after.area.crime.count_12m {
        fields.push("crimeCount12m".to_owned());
    }
    if before.area.crime.rate_per_1k != after.area.crime.rate_per_1k {
        fields.push("crimeRatePer1k".to_owned());
    }
    if before.area.crime.violent_12m != after.area.crime.violent_12m {
        fields.push("crimeViolent12m".to_owned());
    }
    if before.area.crime.burglary_12m != after.area.crime.burglary_12m {
        fields.push("crimeBurglary12m".to_owned());
    }
    if before.area.crime.robbery_12m != after.area.crime.robbery_12m {
        fields.push("crimeRobbery12m".to_owned());
    }
    if before.area.crime.updated_at != after.area.crime.updated_at {
        fields.push("crimeUpdatedAt".to_owned());
    }
    fields
}

#[cfg(test)]
mod tests {
    use std::path::Path;

    use rusqlite::Connection;
    use tempfile::TempDir;
    use uuid::Uuid;

    use crate::schema::listing::{
        Agent, AreaMetrics, ExtractionStatus, GeoLocation, Lettings, Listing, ListingStatus,
        MapViews, PortalIds, RemoteLocalAsset,
    };

    use super::{
        EnrichmentMode, SOURCE_BROADBAND, SOURCE_CENSUS, SOURCE_CRIME, SOURCE_DEPRIVATION,
        SOURCE_FLOOD, SOURCE_INCOME, SOURCE_POPULATION, SOURCE_UPRN, SourceEnricher,
    };

    #[test]
    fn replace_mode_populates_listing_from_sources() {
        let temp = build_test_sources();
        let enricher = SourceEnricher::open(temp.path()).expect("open enricher");
        let mut listing = sample_listing("AA1 1AA");

        let report = enricher
            .enrich_listing(&mut listing, EnrichmentMode::ReplaceFromSources)
            .expect("enrich listing");

        assert_eq!(listing.area.lsoa.code.as_deref(), Some("LSOA001"));
        assert_eq!(listing.area.msoa.code.as_deref(), Some("MSOA001"));
        assert_eq!(listing.gigabit_availability, Some(87.4));
        assert_eq!(listing.area.imd.rank, Some(222));
        assert_eq!(listing.area.imd.decile, Some(9));
        assert_eq!(listing.area.income.bhc, Some(611.2));
        assert_eq!(listing.area.income.ahc, Some(503.0));
        assert_eq!(listing.area.social_housing_pct, Some(14.5));
        assert_eq!(listing.area.population, Some(12_000));
        assert_eq!(listing.area.flood_risk.level.as_deref(), Some("low"));
        assert_eq!(listing.area.crime.count_12m, Some(120));
        assert_eq!(listing.area.crime.rate_per_1k, Some(10.0));
        assert_eq!(listing.area.crime.updated_at.as_deref(), Some("2026-02"));
        assert!(report.missing_categories.is_empty());
        assert!(
            report
                .applied_fields
                .contains(&"gigabitAvailability".to_owned())
        );
        assert!(report.applied_fields.contains(&"crimeRatePer1k".to_owned()));
    }

    #[test]
    fn fill_missing_mode_preserves_manual_values() {
        let temp = build_test_sources();
        let enricher = SourceEnricher::open(temp.path()).expect("open enricher");
        let mut listing = sample_listing("AA1 1AA");
        listing.gigabit_availability = Some(33.3);
        listing.area.imd.decile = Some(4);
        listing.area.population = Some(5_000);
        listing.area.crime.rate_per_1k = Some(9.9);

        enricher
            .enrich_listing(&mut listing, EnrichmentMode::FillMissingFromSources)
            .expect("enrich listing");

        assert_eq!(listing.gigabit_availability, Some(33.3));
        assert_eq!(listing.area.imd.decile, Some(4));
        assert_eq!(listing.area.population, Some(5_000));
        assert_eq!(listing.area.crime.rate_per_1k, Some(9.9));
        assert_eq!(listing.area.income.bhc, Some(611.2));
    }

    #[test]
    fn report_lists_unavailable_sources() {
        let temp = TempDir::new().expect("temp dir");
        seed_postcodes_db(temp.path());
        let enricher = SourceEnricher::open(temp.path()).expect("open enricher");
        let mut listing = sample_listing("AA1 1AA");

        let report = enricher
            .enrich_listing(&mut listing, EnrichmentMode::ReplaceFromSources)
            .expect("enrich listing");

        assert!(
            report
                .unavailable_sources
                .contains(&SOURCE_BROADBAND.to_owned())
        );
        assert!(
            report
                .unavailable_sources
                .contains(&SOURCE_DEPRIVATION.to_owned())
        );
        assert!(
            report
                .unavailable_sources
                .contains(&SOURCE_CENSUS.to_owned())
        );
        assert!(
            report
                .unavailable_sources
                .contains(&SOURCE_POPULATION.to_owned())
        );
        assert!(
            report
                .unavailable_sources
                .contains(&SOURCE_INCOME.to_owned())
        );
        assert!(
            report
                .unavailable_sources
                .contains(&SOURCE_FLOOD.to_owned())
        );
        assert!(
            report
                .unavailable_sources
                .contains(&SOURCE_CRIME.to_owned())
        );
        assert!(report.unavailable_sources.contains(&SOURCE_UPRN.to_owned()));
    }

    fn build_test_sources() -> TempDir {
        let temp = TempDir::new().expect("temp dir");
        seed_postcodes_db(temp.path());
        seed_broadband_db(temp.path());
        seed_deprivation_db(temp.path());
        seed_census_db(temp.path());
        seed_population_db(temp.path());
        seed_income_db(temp.path());
        seed_flood_db(temp.path());
        seed_crime_db(temp.path());
        temp
    }

    fn seed_postcodes_db(root: &Path) {
        let path = root.join("postcodes.db");
        let connection = Connection::open(path).expect("open db");
        connection
            .execute_batch(
                "
                CREATE TABLE postcodes (
                    postcode TEXT PRIMARY KEY,
                    postcode_display TEXT,
                    lat REAL,
                    lng REAL,
                    lsoa_code TEXT,
                    lsoa_name TEXT,
                    msoa_code TEXT,
                    msoa_name TEXT
                );
                INSERT INTO postcodes (postcode, postcode_display, lat, lng, lsoa_code, lsoa_name, msoa_code, msoa_name)
                VALUES ('AA11AA', 'AA1 1AA', 51.5074, -0.1278, 'LSOA001', 'LSOA One', 'MSOA001', 'MSOA One');
                ",
            )
            .expect("seed postcodes");
    }

    fn seed_broadband_db(root: &Path) {
        let path = root.join("broadband.db");
        let connection = Connection::open(path).expect("open db");
        connection
            .execute_batch(
                "
                CREATE TABLE postcodes (
                    postcode TEXT PRIMARY KEY,
                    postcode_display TEXT,
                    gigabit_availability REAL
                );
                INSERT INTO postcodes (postcode, postcode_display, gigabit_availability)
                VALUES ('AA11AA', 'AA1 1AA', 87.4);
                ",
            )
            .expect("seed broadband");
    }

    fn seed_deprivation_db(root: &Path) {
        let path = root.join("deprivation.db");
        let connection = Connection::open(path).expect("open db");
        connection
            .execute_batch(
                "
                CREATE TABLE imd (
                    lsoa_code TEXT PRIMARY KEY,
                    rank INTEGER,
                    decile INTEGER,
                    score REAL
                );
                INSERT INTO imd (lsoa_code, rank, decile, score)
                VALUES ('LSOA001', 222, 9, 7.4);
                ",
            )
            .expect("seed deprivation");
    }

    fn seed_census_db(root: &Path) {
        let path = root.join("census.db");
        let connection = Connection::open(path).expect("open db");
        connection
            .execute_batch(
                "
                CREATE TABLE tenure (
                    lsoa_code TEXT PRIMARY KEY,
                    social_housing_pct REAL
                );
                INSERT INTO tenure (lsoa_code, social_housing_pct)
                VALUES ('LSOA001', 14.5);
                ",
            )
            .expect("seed census");
    }

    fn seed_population_db(root: &Path) {
        let path = root.join("population.db");
        let connection = Connection::open(path).expect("open db");
        connection
            .execute_batch(
                "
                CREATE TABLE population (
                    lsoa_code TEXT PRIMARY KEY,
                    population INTEGER
                );
                INSERT INTO population (lsoa_code, population)
                VALUES ('LSOA001', 12000);
                ",
            )
            .expect("seed population");
    }

    fn seed_income_db(root: &Path) {
        let path = root.join("income.db");
        let connection = Connection::open(path).expect("open db");
        connection
            .execute_batch(
                "
                CREATE TABLE income (
                    msoa_code TEXT PRIMARY KEY,
                    income_bhc REAL,
                    income_ahc REAL
                );
                INSERT INTO income (msoa_code, income_bhc, income_ahc)
                VALUES ('MSOA001', 611.2, 503.0);
                ",
            )
            .expect("seed income");
    }

    fn seed_flood_db(root: &Path) {
        let path = root.join("flood.db");
        let connection = Connection::open(path).expect("open db");
        connection
            .execute_batch(
                "
                CREATE TABLE flood (
                    postcode TEXT PRIMARY KEY,
                    risk TEXT,
                    source TEXT
                );
                INSERT INTO flood (postcode, risk, source)
                VALUES ('AA11AA', 'low', 'ea-postcode-risk');
                ",
            )
            .expect("seed flood");
    }

    fn seed_crime_db(root: &Path) {
        let path = root.join("crime.db");
        let connection = Connection::open(path).expect("open db");
        connection
            .execute_batch(
                "
                CREATE TABLE crime_12m (
                    lsoa_code TEXT PRIMARY KEY,
                    total INTEGER,
                    violent INTEGER,
                    burglary INTEGER,
                    robbery INTEGER,
                    month_end TEXT
                );
                INSERT INTO crime_12m (lsoa_code, total, violent, burglary, robbery, month_end)
                VALUES ('LSOA001', 120, 18, 9, 3, '2026-02');
                ",
            )
            .expect("seed crime");
    }

    fn seed_uprn_db(root: &Path) {
        let path = root.join("uprn.db");
        let connection = Connection::open(path).expect("open db");
        connection
            .execute_batch(
                "
                CREATE TABLE uprn (
                    uprn TEXT PRIMARY KEY,
                    lat REAL,
                    lng REAL,
                    x REAL,
                    y REAL
                );
                INSERT INTO uprn (uprn, lat, lng, x, y)
                VALUES
                    ('100021234567', 51.50741, -0.12779, NULL, NULL),
                    ('100021234568', 51.50760, -0.12760, NULL, NULL);
                ",
            )
            .expect("seed uprn");
    }

    #[test]
    fn lookup_postcode_coordinates_returns_lat_lng() {
        let temp = build_test_sources();
        let enricher = SourceEnricher::open(temp.path()).expect("open enricher");

        let result = enricher
            .lookup_postcode_coordinates("AA11AA")
            .expect("query should succeed");

        let coords = result.expect("should find coordinates");
        assert!((coords.lat - 51.5074).abs() < 0.001);
        assert!((coords.lng - (-0.1278)).abs() < 0.001);
    }

    #[test]
    fn lookup_postcode_coordinates_returns_none_for_unknown() {
        let temp = build_test_sources();
        let enricher = SourceEnricher::open(temp.path()).expect("open enricher");

        let result = enricher
            .lookup_postcode_coordinates("ZZ99ZZ")
            .expect("query should succeed");

        assert!(result.is_none());
    }

    #[test]
    fn lookup_uprn_candidates_returns_nearest_sorted_matches() {
        let temp = build_test_sources();
        seed_uprn_db(temp.path());
        let enricher = SourceEnricher::open(temp.path()).expect("open enricher");

        let result = enricher
            .lookup_uprn_candidates(51.5074, -0.1278, 30.0, 5)
            .expect("query should succeed");

        assert_eq!(result.len(), 2);
        assert_eq!(result[0].uprn, "100021234567");
        assert!(result[0].distance_m < result[1].distance_m);
    }

    fn sample_listing(postcode: &str) -> Listing {
        Listing {
            id: Uuid::new_v4().to_string(),
            portal_ids: PortalIds::default(),
            uprn: None,
            uprn_source: None,
            uprn_confidence: None,
            url: "https://example.com/listing".to_owned(),
            location: GeoLocation {
                lat: 52.0,
                lng: -2.0,
                pin_type: None,
            },
            postcode: postcode.to_owned(),
            address: "1 Example Street".to_owned(),
            region: Some("Region".to_owned()),
            google_maps_url: "https://maps.google.com".to_owned(),
            google_maps_street_view_url: "https://maps.google.com/street".to_owned(),
            area: AreaMetrics::default(),
            price: 1200,
            price_display: "£1,200 pcm".to_owned(),
            bedrooms: 2,
            bathrooms: 1,
            property_type: "Flat".to_owned(),
            description: "test".to_owned(),
            notes: Vec::new(),
            images: Vec::new(),
            floorplan: RemoteLocalAsset::default(),
            epc: RemoteLocalAsset::default(),
            map_views: MapViews::default(),
            epc_rating: None,
            floor_area_sqm: None,
            epc_lodgement_date: None,
            epc_address_match: None,
            epc_search_url: None,
            nearest_stations: Vec::new(),
            gigabit_availability: None,
            listed_date: None,
            lettings: Lettings::default(),
            agent: Agent::default(),
            assessment: None,
            assessed_at: None,
            assessed_score: None,
            scores: None,
            fetched_at: "2026-03-10T00:00:00.000Z".to_owned(),
            extraction_status: ExtractionStatus::Success,
            status: ListingStatus::Active,
            notion_page_id: None,
        }
    }
}
