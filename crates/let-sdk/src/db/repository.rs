#![forbid(unsafe_code)]

use std::collections::HashSet;
use std::env;
use std::fs;
use std::path::Path;
use std::time::Duration;

use rusqlite::{Connection, OptionalExtension, Row, Transaction, named_params, params};
use serde::Serialize;
use serde::de::DeserializeOwned;
use serde_json::Value;
use uuid::Uuid;

use crate::db::{close_listings_db, open_listings_db, open_listings_db_readonly};
use crate::errors::{ErrorCode, LetError, Result};
use crate::schema::listing::{
    Agent, AreaCodeName, AreaMetrics, CrimeMetrics, ExtractionStatus, FloodRisk, GeoLocation,
    ImdMetrics, IncomeMetrics, Lettings, Listing, ListingAssessment, ListingImage, ListingStatus,
    ListingsFile, MapViews, PortalIds, RemoteLocalAsset, ScoreContext, ScoreFactors,
    ScorePenalties, ScorePercentiles, Scores, StationDistance, StatsSummary,
};

const DEFAULT_UPDATED_AT: &str = "1970-01-01T00:00:00.000Z";

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DbMeta {
    pub updated_at: String,
    pub last_search_total: i64,
}

pub fn load_listings_file(path: impl AsRef<Path>) -> Result<ListingsFile> {
    let connection = open_listings_db_readonly(path)?;
    let result = (|| {
        let meta = load_meta(&connection)?;
        let search_urls =
            load_string_column(&connection, "SELECT url FROM search_urls ORDER BY url")?;
        let locations = load_string_column(
            &connection,
            "SELECT name FROM search_locations ORDER BY name",
        )?;
        let listings = load_all_listings(&connection)?;

        Ok(ListingsFile {
            updated_at: meta.updated_at,
            search_urls,
            locations,
            last_search_total: meta.last_search_total,
            listings,
        })
    })();

    finalize_connection(connection, result)
}

pub fn upsert_listings(
    path: impl AsRef<Path>,
    new_listings: &[Listing],
    updated_listings: &[Listing],
    all_scored_listings: &[Listing],
    meta: &DbMeta,
    search_urls: &[String],
    locations: &[String],
) -> Result<()> {
    let db_path = path.as_ref();
    backup_listings_db(db_path)?;

    let mut connection = open_listings_db(db_path)?;
    let result = (|| {
        let tx = connection.transaction()?;

        for listing in updated_listings {
            tx.execute("DELETE FROM listings WHERE id = ?1", params![listing.id])?;
        }

        for listing in new_listings.iter().chain(updated_listings) {
            insert_listing_graph(&tx, listing)?;
        }

        for listing in all_scored_listings {
            tx.execute(
                "DELETE FROM scores WHERE listing_id = ?1",
                params![listing.id],
            )?;
            tx.execute(
                "DELETE FROM score_contexts WHERE listing_id = ?1",
                params![listing.id],
            )?;
            insert_score_row(&tx, listing)?;
        }

        let mut new_or_updated_ids = HashSet::new();
        for listing in new_listings.iter().chain(updated_listings) {
            new_or_updated_ids.insert(listing.id.as_str());
        }
        update_unchanged_assessed_scores(&tx, all_scored_listings, &new_or_updated_ids)?;

        tx.execute("DELETE FROM meta", [])?;
        tx.execute(
            "INSERT INTO meta (id, updated_at, last_search_total) VALUES (1, ?1, ?2)",
            params![meta.updated_at, meta.last_search_total],
        )?;

        tx.execute("DELETE FROM search_urls", [])?;
        for url in search_urls {
            tx.execute("INSERT INTO search_urls (url) VALUES (?1)", params![url])?;
        }

        tx.execute("DELETE FROM search_locations", [])?;
        for name in locations {
            tx.execute(
                "INSERT INTO search_locations (name) VALUES (?1)",
                params![name],
            )?;
        }

        tx.commit()?;
        Ok(())
    })();

    finalize_connection(connection, result)
}

pub fn update_listing_assessment(
    path: impl AsRef<Path>,
    listing_id: &str,
    assessment: &ListingAssessment,
    assessed_score: f64,
    assessed_at: &str,
) -> Result<()> {
    let mut connection = open_listings_db(path)?;
    let result = (|| {
        let tx = connection.transaction()?;
        let updated = tx.execute(
            "UPDATE listings SET assessed_at = ?1, assessed_score = ?2 WHERE id = ?3",
            params![assessed_at, assessed_score, listing_id],
        )?;

        if updated == 0 {
            return Err(LetError::new(
                ErrorCode::NotFound,
                format!("listing not found: {listing_id}"),
                "ensure the listing exists before submitting assessment data",
            ));
        }

        tx.execute(
            "DELETE FROM assessments WHERE listing_id = ?1",
            params![listing_id],
        )?;
        insert_assessment_row(&tx, listing_id, assessment)?;

        tx.commit()?;
        Ok(())
    })();

    finalize_connection(connection, result)
}

pub fn find_listing_by_id_from_db(path: impl AsRef<Path>, id: &str) -> Result<Option<Listing>> {
    let connection = open_listings_db_readonly(path)?;
    let result = find_listing_by_id_with_connection(&connection, id);
    finalize_connection(connection, result)
}

fn finalize_connection<T>(connection: Connection, result: Result<T>) -> Result<T> {
    let close_result = close_listings_db(connection);

    match (result, close_result) {
        (Err(error), _) => Err(error),
        (Ok(_value), Err(error)) => Err(error),
        (Ok(value), Ok(())) => Ok(value),
    }
}

fn backup_listings_db(path: &Path) -> Result<()> {
    if !path.exists() {
        return Ok(());
    }

    if env::var("LET_SKIP_DB_BACKUP")
        .map(|value| value == "1")
        .unwrap_or(false)
    {
        return Ok(());
    }

    let backup_path = format!("{}.bak", path.display());
    let min_interval_secs = env::var("LET_DB_BACKUP_MIN_SECONDS")
        .ok()
        .and_then(|value| value.parse::<u64>().ok())
        .unwrap_or(300);

    if min_interval_secs > 0
        && let Ok(metadata) = fs::metadata(&backup_path)
        && let Ok(modified_at) = metadata.modified()
        && let Ok(elapsed) = modified_at.elapsed()
        && elapsed < Duration::from_secs(min_interval_secs)
    {
        return Ok(());
    }

    fs::copy(path, backup_path)?;
    Ok(())
}

fn load_meta(connection: &Connection) -> Result<DbMeta> {
    let maybe_meta = connection
        .query_row(
            "SELECT updated_at, last_search_total FROM meta WHERE id = 1",
            [],
            |row| {
                Ok(DbMeta {
                    updated_at: row.get(0)?,
                    last_search_total: row.get(1)?,
                })
            },
        )
        .optional()?;

    Ok(maybe_meta.unwrap_or(DbMeta {
        updated_at: DEFAULT_UPDATED_AT.to_owned(),
        last_search_total: 0,
    }))
}

fn load_string_column(connection: &Connection, sql: &str) -> Result<Vec<String>> {
    let mut statement = connection.prepare(sql)?;
    let rows = statement.query_map([], |row| row.get::<_, String>(0))?;
    let values = rows.collect::<std::result::Result<Vec<_>, rusqlite::Error>>()?;
    Ok(values)
}

fn load_all_listings(connection: &Connection) -> Result<Vec<Listing>> {
    let has_score_contexts = table_exists(connection, "score_contexts")?;
    let mut statement = connection.prepare("SELECT * FROM listings ORDER BY id")?;
    let rows = statement.query_map([], ListingRow::from_row)?;
    let listing_rows = rows.collect::<std::result::Result<Vec<_>, rusqlite::Error>>()?;

    listing_rows
        .into_iter()
        .map(|row| hydrate_listing(connection, row, has_score_contexts))
        .collect()
}

fn find_listing_by_id_with_connection(
    connection: &Connection,
    id: &str,
) -> Result<Option<Listing>> {
    let has_score_contexts = table_exists(connection, "score_contexts")?;
    let sql = if Uuid::parse_str(id).is_ok() {
        "SELECT * FROM listings WHERE id = ?1"
    } else {
        "SELECT * FROM listings WHERE portal_rightmove = ?1"
    };

    let row = connection
        .query_row(sql, params![id], ListingRow::from_row)
        .optional()?;

    row.map(|item| hydrate_listing(connection, item, has_score_contexts))
        .transpose()
}

fn hydrate_listing(
    connection: &Connection,
    row: ListingRow,
    has_score_contexts: bool,
) -> Result<Listing> {
    let listing_id = row.id.clone();
    let notes = load_notes(connection, &listing_id)?;
    let images = load_images(connection, &listing_id)?;
    let stations = load_stations(connection, &listing_id)?;
    let scores = load_scores(connection, &listing_id, has_score_contexts)?;
    let assessment = load_assessment(connection, &listing_id)?;

    let extraction_status: ExtractionStatus =
        parse_required_enum(&row.extraction_status, "listings.extraction_status")?;
    let status: ListingStatus = parse_required_enum(&row.status, "listings.status")?;

    Ok(Listing {
        id: row.id,
        portal_ids: PortalIds {
            rightmove: row.portal_rightmove,
            zoopla: row.portal_zoopla,
            onthemarket: row.portal_onthemarket,
        },
        uprn: row.uprn,
        uprn_source: parse_optional_enum(row.uprn_source, "listings.uprn_source")?,
        uprn_confidence: parse_optional_enum(row.uprn_confidence, "listings.uprn_confidence")?,
        url: row.url,
        location: GeoLocation {
            lat: row.lat,
            lng: row.lng,
            pin_type: parse_optional_enum(row.pin_type, "listings.pin_type")?,
        },
        postcode: row.postcode,
        address: row.address,
        region: row.region,
        google_maps_url: row.google_maps_url,
        google_maps_street_view_url: row.google_maps_street_view_url,
        area: AreaMetrics {
            lsoa: AreaCodeName {
                code: row.area_lsoa_code,
                name: row.area_lsoa_name,
            },
            msoa: AreaCodeName {
                code: row.area_msoa_code,
                name: row.area_msoa_name,
            },
            imd: ImdMetrics {
                rank: row.imd_rank,
                decile: row.imd_decile,
                score: row.imd_score,
            },
            income: IncomeMetrics {
                bhc: row.income_bhc,
                ahc: row.income_ahc,
            },
            social_housing_pct: row.social_housing_pct,
            population: row.population,
            flood_risk: FloodRisk {
                level: row.flood_risk_level,
                source: row.flood_risk_source,
            },
            crime: CrimeMetrics {
                count_12m: row.crime_count_12m,
                rate_per_1k: row.crime_rate_per_1k,
                violent_12m: row.crime_violent_12m,
                burglary_12m: row.crime_burglary_12m,
                robbery_12m: row.crime_robbery_12m,
                band: parse_optional_enum(row.crime_band, "listings.crime_band")?,
                trend: parse_optional_enum(row.crime_trend, "listings.crime_trend")?,
                updated_at: row.crime_updated_at,
            },
        },
        price: row.price,
        price_display: row.price_display,
        bedrooms: row.bedrooms,
        bathrooms: row.bathrooms,
        property_type: row.property_type,
        description: row.description,
        notes,
        images,
        floorplan: RemoteLocalAsset {
            remote: row.floorplan_remote,
            local: row.floorplan_local,
        },
        epc: RemoteLocalAsset {
            remote: row.epc_remote,
            local: row.epc_local,
        },
        map_views: MapViews {
            satellite: RemoteLocalAsset {
                remote: row.map_satellite_remote,
                local: row.map_satellite_local,
            },
            street: RemoteLocalAsset {
                remote: row.map_street_remote,
                local: row.map_street_local,
            },
        },
        epc_rating: parse_optional_enum(row.epc_rating, "listings.epc_rating")?,
        floor_area_sqm: row.floor_area_sqm,
        epc_lodgement_date: row.epc_lodgement_date,
        epc_address_match: row.epc_address_match.map(|value| value == 1),
        epc_search_url: row.epc_search_url,
        nearest_stations: stations,
        gigabit_availability: row.gigabit_availability,
        listed_date: row.listed_date,
        lettings: Lettings {
            available_date: row.available_date,
            deposit: parse_deposit(row.deposit)?,
        },
        agent: Agent {
            name: row.agent_name,
            phone: row.agent_phone,
        },
        assessment,
        assessed_at: row.assessed_at,
        assessed_score: row.assessed_score,
        scores,
        fetched_at: row.fetched_at,
        extraction_status,
        status,
        notion_page_id: row.notion_page_id,
    })
}

fn load_notes(connection: &Connection, listing_id: &str) -> Result<Vec<String>> {
    let mut statement =
        connection.prepare("SELECT note FROM notes WHERE listing_id = ?1 ORDER BY position")?;
    let rows = statement.query_map(params![listing_id], |row| row.get::<_, String>(0))?;
    let values = rows.collect::<std::result::Result<Vec<_>, rusqlite::Error>>()?;
    Ok(values)
}

fn load_images(connection: &Connection, listing_id: &str) -> Result<Vec<ListingImage>> {
    let mut statement = connection
        .prepare("SELECT remote, local FROM images WHERE listing_id = ?1 ORDER BY position")?;
    let rows = statement.query_map(params![listing_id], |row| {
        Ok(ListingImage {
            remote: row.get(0)?,
            local: row.get(1)?,
        })
    })?;
    let values = rows.collect::<std::result::Result<Vec<_>, rusqlite::Error>>()?;
    Ok(values)
}

fn load_stations(connection: &Connection, listing_id: &str) -> Result<Vec<StationDistance>> {
    let mut statement = connection.prepare(
        "SELECT name, distance, unit FROM stations WHERE listing_id = ?1 ORDER BY position",
    )?;
    let rows = statement.query_map(params![listing_id], |row| {
        Ok(StationDistance {
            name: row.get(0)?,
            distance: row.get(1)?,
            unit: row.get(2)?,
        })
    })?;
    let values = rows.collect::<std::result::Result<Vec<_>, rusqlite::Error>>()?;
    Ok(values)
}

fn load_scores(
    connection: &Connection,
    listing_id: &str,
    has_score_contexts: bool,
) -> Result<Option<Scores>> {
    let row = connection
        .query_row(
            "SELECT * FROM scores WHERE listing_id = ?1",
            params![listing_id],
            ScoreRow::from_row,
        )
        .optional()?;

    let Some(row) = row else {
        return Ok(None);
    };

    let context = load_score_context(connection, listing_id, has_score_contexts)?;
    decode_scores(row, context).map(Some)
}

fn load_assessment(connection: &Connection, listing_id: &str) -> Result<Option<ListingAssessment>> {
    let row = connection
        .query_row(
            "SELECT * FROM assessments WHERE listing_id = ?1",
            params![listing_id],
            AssessmentRow::from_row,
        )
        .optional()?;

    row.map(decode_assessment).transpose()
}

fn decode_scores(row: ScoreRow, context: Option<ScoreContext>) -> Result<Scores> {
    Ok(Scores {
        overall: row.overall,
        confidence: row.confidence,
        affordability: row.affordability,
        location: row.location,
        liveability: row.liveability,
        factors: ScoreFactors {
            monthly_rent: row.factor_monthly_rent,
            price_percentile: row.factor_price_percentile,
            floor_area_sqm: row.factor_floor_area_sqm,
            floor_area_percentile: row.factor_floor_area_percentile,
            epc_band: row.factor_epc_band,
            epc_numeric: row.factor_epc_numeric,
            true_monthly_cost: row.factor_true_monthly_cost,
            true_cost_percentile: row.factor_true_cost_percentile,
            station_miles: row.factor_station_miles,
            station_percentile: row.factor_station_percentile,
            gigabit_pct: row.factor_gigabit_pct,
            region_name: row.factor_region_name,
            priority_score: row.factor_priority_score,
            garden_type: parse_required_enum(&row.factor_garden_type, "scores.factor_garden_type")?,
            heating_type: parse_required_enum(
                &row.factor_heating_type,
                "scores.factor_heating_type",
            )?,
            pet_policy: parse_required_enum(&row.factor_pet_policy, "scores.factor_pet_policy")?,
            property_type: row.factor_property_type,
            bedrooms: row.factor_bedrooms,
            imd_decile: row.factor_imd_decile,
            crime_rate_per_1k: row.factor_crime_rate_per_1k,
            crime_rate_percentile: row.factor_crime_rate_percentile,
        },
        penalties: ScorePenalties {
            epc: row.penalty_epc,
            garden: row.penalty_garden,
            pets: row.penalty_pets,
            combined: row.penalty_combined,
        },
        context: context.unwrap_or_else(legacy_score_context),
    })
}

fn decode_assessment(row: AssessmentRow) -> Result<ListingAssessment> {
    Ok(ListingAssessment {
        maintenance: parse_required_enum(&row.maintenance, "assessments.maintenance")?,
        light_and_space: row.light_and_space,
        photo_analysis: row.photo_analysis,
        tradeoffs: row.tradeoffs,
        neighborhood_analysis: row.neighborhood_analysis,
        recommendation: parse_required_enum(&row.recommendation, "assessments.recommendation")?,
        family_suitability: parse_required_enum(
            &row.family_suitability,
            "assessments.family_suitability",
        )?,
        reasoning: row.reasoning,
        score_adjustment: row.score_adjustment,
    })
}

fn load_score_context(
    connection: &Connection,
    listing_id: &str,
    has_score_contexts: bool,
) -> Result<Option<ScoreContext>> {
    if !has_score_contexts {
        return Ok(None);
    }

    let raw = connection
        .query_row(
            "SELECT context_json FROM score_contexts WHERE listing_id = ?1",
            params![listing_id],
            |row| row.get::<_, String>(0),
        )
        .optional()?;

    raw.map(|value| decode_score_context(&value)).transpose()
}

fn decode_score_context(raw: &str) -> Result<ScoreContext> {
    serde_json::from_str::<ScoreContext>(raw).map_err(|error| {
        LetError::new(
            ErrorCode::SchemaMismatch,
            format!("invalid score context JSON: {error}"),
            "rebuild or migrate the listings database to match current schema",
        )
    })
}

fn legacy_score_context() -> ScoreContext {
    ScoreContext {
        config_hash: "legacy".to_owned(),
        percentiles: ScorePercentiles {
            prices: zero_stats(),
            true_costs: zero_stats(),
            floor_areas: zero_stats(),
            station_distances: zero_stats(),
            crime_rates: zero_stats(),
        },
    }
}

fn zero_stats() -> StatsSummary {
    StatsSummary {
        min: 0.0,
        max: 0.0,
        mean: 0.0,
        median: 0.0,
        std_dev: 0.0,
    }
}

fn insert_listing_graph(tx: &Transaction<'_>, listing: &Listing) -> Result<()> {
    insert_listing_row(tx, listing)?;
    insert_station_rows(tx, listing)?;
    insert_image_rows(tx, listing)?;
    insert_note_rows(tx, listing)?;
    if let Some(assessment) = listing.assessment.as_ref() {
        insert_assessment_row(tx, listing.id.as_str(), assessment)?;
    }
    Ok(())
}

fn insert_listing_row(tx: &Transaction<'_>, listing: &Listing) -> Result<()> {
    let uprn_source = encode_optional_enum(listing.uprn_source.as_ref())?;
    let uprn_confidence = encode_optional_enum(listing.uprn_confidence.as_ref())?;
    let pin_type = encode_optional_enum(listing.location.pin_type.as_ref())?;
    let epc_rating = encode_optional_enum(listing.epc_rating.as_ref())?;
    let extraction_status = encode_enum(&listing.extraction_status)?;
    let status = encode_enum(&listing.status)?;
    let crime_band = encode_optional_enum(listing.area.crime.band.as_ref())?;
    let crime_trend = encode_optional_enum(listing.area.crime.trend.as_ref())?;
    let epc_address_match = listing
        .epc_address_match
        .map(|value| if value { 1 } else { 0 });
    let deposit = listing.lettings.deposit;

    tx.execute(
        "INSERT INTO listings (
            id, portal_rightmove, portal_zoopla, portal_onthemarket,
            uprn, uprn_source, uprn_confidence,
            url, address, postcode, region, lat, lng, pin_type,
            google_maps_url, google_maps_street_view_url,
            price, price_display, bedrooms, bathrooms, property_type,
            description, floorplan_remote, floorplan_local, epc_remote, epc_local,
            map_satellite_remote, map_satellite_local, map_street_remote, map_street_local,
            epc_rating, floor_area_sqm, epc_lodgement_date, epc_address_match, epc_search_url,
            gigabit_availability, listed_date, available_date, deposit,
            agent_name, agent_phone,
            area_lsoa_code, area_lsoa_name, area_msoa_code, area_msoa_name,
            imd_rank, imd_decile, imd_score, income_bhc, income_ahc,
            social_housing_pct, population, flood_risk_level, flood_risk_source,
            crime_count_12m, crime_rate_per_1k, crime_violent_12m, crime_burglary_12m, crime_robbery_12m,
            crime_band, crime_trend, crime_updated_at,
            fetched_at, extraction_status, status, notion_page_id,
            assessed_at, assessed_score
        ) VALUES (
            :id, :portal_rightmove, :portal_zoopla, :portal_onthemarket,
            :uprn, :uprn_source, :uprn_confidence,
            :url, :address, :postcode, :region, :lat, :lng, :pin_type,
            :google_maps_url, :google_maps_street_view_url,
            :price, :price_display, :bedrooms, :bathrooms, :property_type,
            :description, :floorplan_remote, :floorplan_local, :epc_remote, :epc_local,
            :map_satellite_remote, :map_satellite_local, :map_street_remote, :map_street_local,
            :epc_rating, :floor_area_sqm, :epc_lodgement_date, :epc_address_match, :epc_search_url,
            :gigabit_availability, :listed_date, :available_date, :deposit,
            :agent_name, :agent_phone,
            :area_lsoa_code, :area_lsoa_name, :area_msoa_code, :area_msoa_name,
            :imd_rank, :imd_decile, :imd_score, :income_bhc, :income_ahc,
            :social_housing_pct, :population, :flood_risk_level, :flood_risk_source,
            :crime_count_12m, :crime_rate_per_1k, :crime_violent_12m, :crime_burglary_12m, :crime_robbery_12m,
            :crime_band, :crime_trend, :crime_updated_at,
            :fetched_at, :extraction_status, :status, :notion_page_id,
            :assessed_at, :assessed_score
        )",
        named_params! {
            ":id": listing.id,
            ":portal_rightmove": listing.portal_ids.rightmove,
            ":portal_zoopla": listing.portal_ids.zoopla,
            ":portal_onthemarket": listing.portal_ids.onthemarket,
            ":uprn": listing.uprn,
            ":uprn_source": uprn_source,
            ":uprn_confidence": uprn_confidence,
            ":url": listing.url,
            ":address": listing.address,
            ":postcode": listing.postcode,
            ":region": listing.region,
            ":lat": listing.location.lat,
            ":lng": listing.location.lng,
            ":pin_type": pin_type,
            ":google_maps_url": listing.google_maps_url,
            ":google_maps_street_view_url": listing.google_maps_street_view_url,
            ":price": listing.price,
            ":price_display": listing.price_display,
            ":bedrooms": listing.bedrooms,
            ":bathrooms": listing.bathrooms,
            ":property_type": listing.property_type,
            ":description": listing.description,
            ":floorplan_remote": listing.floorplan.remote,
            ":floorplan_local": listing.floorplan.local,
            ":epc_remote": listing.epc.remote,
            ":epc_local": listing.epc.local,
            ":map_satellite_remote": listing.map_views.satellite.remote,
            ":map_satellite_local": listing.map_views.satellite.local,
            ":map_street_remote": listing.map_views.street.remote,
            ":map_street_local": listing.map_views.street.local,
            ":epc_rating": epc_rating,
            ":floor_area_sqm": listing.floor_area_sqm,
            ":epc_lodgement_date": listing.epc_lodgement_date,
            ":epc_address_match": epc_address_match,
            ":epc_search_url": listing.epc_search_url,
            ":gigabit_availability": listing.gigabit_availability,
            ":listed_date": listing.listed_date,
            ":available_date": listing.lettings.available_date,
            ":deposit": deposit,
            ":agent_name": listing.agent.name,
            ":agent_phone": listing.agent.phone,
            ":area_lsoa_code": listing.area.lsoa.code,
            ":area_lsoa_name": listing.area.lsoa.name,
            ":area_msoa_code": listing.area.msoa.code,
            ":area_msoa_name": listing.area.msoa.name,
            ":imd_rank": listing.area.imd.rank,
            ":imd_decile": listing.area.imd.decile,
            ":imd_score": listing.area.imd.score,
            ":income_bhc": listing.area.income.bhc,
            ":income_ahc": listing.area.income.ahc,
            ":social_housing_pct": listing.area.social_housing_pct,
            ":population": listing.area.population,
            ":flood_risk_level": listing.area.flood_risk.level,
            ":flood_risk_source": listing.area.flood_risk.source,
            ":crime_count_12m": listing.area.crime.count_12m,
            ":crime_rate_per_1k": listing.area.crime.rate_per_1k,
            ":crime_violent_12m": listing.area.crime.violent_12m,
            ":crime_burglary_12m": listing.area.crime.burglary_12m,
            ":crime_robbery_12m": listing.area.crime.robbery_12m,
            ":crime_band": crime_band,
            ":crime_trend": crime_trend,
            ":crime_updated_at": listing.area.crime.updated_at,
            ":fetched_at": listing.fetched_at,
            ":extraction_status": extraction_status,
            ":status": status,
            ":notion_page_id": listing.notion_page_id,
            ":assessed_at": listing.assessed_at,
            ":assessed_score": listing.assessed_score,
        },
    )?;

    Ok(())
}

fn insert_station_rows(tx: &Transaction<'_>, listing: &Listing) -> Result<()> {
    for (position, station) in listing.nearest_stations.iter().enumerate() {
        tx.execute(
            "INSERT INTO stations (listing_id, name, distance, unit, position) VALUES (?1, ?2, ?3, ?4, ?5)",
            params![
                listing.id,
                station.name,
                station.distance,
                station.unit,
                position as i64
            ],
        )?;
    }
    Ok(())
}

fn insert_image_rows(tx: &Transaction<'_>, listing: &Listing) -> Result<()> {
    for (position, image) in listing.images.iter().enumerate() {
        tx.execute(
            "INSERT INTO images (listing_id, remote, local, position) VALUES (?1, ?2, ?3, ?4)",
            params![listing.id, image.remote, image.local, position as i64],
        )?;
    }
    Ok(())
}

fn insert_note_rows(tx: &Transaction<'_>, listing: &Listing) -> Result<()> {
    for (position, note) in listing.notes.iter().enumerate() {
        tx.execute(
            "INSERT INTO notes (listing_id, note, position) VALUES (?1, ?2, ?3)",
            params![listing.id, note, position as i64],
        )?;
    }
    Ok(())
}

fn insert_score_row(tx: &Transaction<'_>, listing: &Listing) -> Result<()> {
    let Some(score) = listing.scores.as_ref() else {
        return Ok(());
    };

    let garden_type = encode_enum(&score.factors.garden_type)?;
    let heating_type = encode_enum(&score.factors.heating_type)?;
    let pet_policy = encode_enum(&score.factors.pet_policy)?;

    tx.execute(
        "INSERT INTO scores (
            listing_id, overall, confidence, affordability, location, liveability,
            penalty_epc, penalty_garden, penalty_pets, penalty_combined,
            factor_monthly_rent, factor_price_percentile, factor_floor_area_sqm, factor_floor_area_percentile,
            factor_epc_band, factor_epc_numeric, factor_true_monthly_cost, factor_true_cost_percentile,
            factor_station_miles, factor_station_percentile, factor_gigabit_pct, factor_region_name,
            factor_priority_score, factor_imd_decile, factor_crime_rate_per_1k, factor_crime_rate_percentile,
            factor_garden_type, factor_heating_type, factor_pet_policy, factor_property_type, factor_bedrooms
        ) VALUES (
            ?1, ?2, ?3, ?4, ?5, ?6,
            ?7, ?8, ?9, ?10,
            ?11, ?12, ?13, ?14,
            ?15, ?16, ?17, ?18,
            ?19, ?20, ?21, ?22,
            ?23, ?24, ?25, ?26,
            ?27, ?28, ?29, ?30, ?31
        )",
        params![
            listing.id,
            score.overall,
            score.confidence,
            score.affordability,
            score.location,
            score.liveability,
            score.penalties.epc,
            score.penalties.garden,
            score.penalties.pets,
            score.penalties.combined,
            score.factors.monthly_rent,
            score.factors.price_percentile,
            score.factors.floor_area_sqm,
            score.factors.floor_area_percentile,
            score.factors.epc_band,
            score.factors.epc_numeric,
            score.factors.true_monthly_cost,
            score.factors.true_cost_percentile,
            score.factors.station_miles,
            score.factors.station_percentile,
            score.factors.gigabit_pct,
            score.factors.region_name,
            score.factors.priority_score,
            score.factors.imd_decile,
            score.factors.crime_rate_per_1k,
            score.factors.crime_rate_percentile,
            garden_type,
            heating_type,
            pet_policy,
            score.factors.property_type,
            score.factors.bedrooms
        ],
    )?;

    let context_json = serde_json::to_string(&score.context).map_err(|error| {
        LetError::new(
            ErrorCode::Internal,
            format!("failed to serialize score context for sqlite: {error}"),
            "verify score context serde attributes and schema compatibility",
        )
    })?;
    tx.execute(
        "INSERT OR REPLACE INTO score_contexts (listing_id, context_json) VALUES (?1, ?2)",
        params![listing.id, context_json],
    )?;

    Ok(())
}

fn insert_assessment_row(
    tx: &Transaction<'_>,
    listing_id: &str,
    assessment: &ListingAssessment,
) -> Result<()> {
    let maintenance = encode_enum(&assessment.maintenance)?;
    let recommendation = encode_enum(&assessment.recommendation)?;
    let family_suitability = encode_enum(&assessment.family_suitability)?;

    tx.execute(
        "INSERT INTO assessments (
            listing_id, maintenance, light_and_space, photo_analysis, tradeoffs,
            neighborhood_analysis, recommendation, family_suitability, reasoning, score_adjustment
        ) VALUES (
            ?1, ?2, ?3, ?4, ?5,
            ?6, ?7, ?8, ?9, ?10
        )",
        params![
            listing_id,
            maintenance,
            assessment.light_and_space,
            assessment.photo_analysis,
            assessment.tradeoffs,
            assessment.neighborhood_analysis,
            recommendation,
            family_suitability,
            assessment.reasoning,
            assessment.score_adjustment,
        ],
    )?;

    Ok(())
}

fn update_unchanged_assessed_scores(
    tx: &Transaction<'_>,
    all_scored_listings: &[Listing],
    new_or_updated_ids: &HashSet<&str>,
) -> Result<()> {
    for listing in all_scored_listings {
        if listing.assessed_at.is_some() && !new_or_updated_ids.contains(listing.id.as_str()) {
            tx.execute(
                "UPDATE listings SET assessed_score = ?1 WHERE id = ?2",
                params![listing.assessed_score, listing.id],
            )?;
        }
    }
    Ok(())
}

fn parse_required_enum<T>(raw: &str, field: &str) -> Result<T>
where
    T: DeserializeOwned,
{
    parse_enum(raw).ok_or_else(|| {
        LetError::new(
            ErrorCode::SchemaMismatch,
            format!("invalid enum value `{raw}` for {field}"),
            "rebuild or migrate the listings database to match current schema",
        )
    })
}

fn parse_optional_enum<T>(raw: Option<String>, field: &str) -> Result<Option<T>>
where
    T: DeserializeOwned,
{
    match raw {
        None => Ok(None),
        Some(value) => parse_enum(value.as_str()).map(Some).ok_or_else(|| {
            LetError::new(
                ErrorCode::SchemaMismatch,
                format!("invalid enum value `{value}` for {field}"),
                "rebuild or migrate the listings database to match current schema",
            )
        }),
    }
}

fn parse_enum<T>(raw: &str) -> Option<T>
where
    T: DeserializeOwned,
{
    serde_json::from_value(Value::String(raw.to_owned())).ok()
}

fn encode_enum<T>(value: &T) -> Result<String>
where
    T: Serialize,
{
    let serialized = serde_json::to_value(value).map_err(|error| {
        LetError::new(
            ErrorCode::Internal,
            format!("failed to serialize enum for sqlite: {error}"),
            "verify enum serde attributes and schema compatibility",
        )
    })?;

    match serialized {
        Value::String(text) => Ok(text),
        _ => Err(LetError::new(
            ErrorCode::Internal,
            "enum serialization did not produce a string".to_owned(),
            "ensure enum serde representation is string-based",
        )),
    }
}

fn encode_optional_enum<T>(value: Option<&T>) -> Result<Option<String>>
where
    T: Serialize,
{
    value.map(encode_enum).transpose()
}

fn parse_deposit(raw: Option<f64>) -> Result<Option<i64>> {
    let Some(value) = raw else {
        return Ok(None);
    };

    if !value.is_finite() {
        return Err(LetError::new(
            ErrorCode::SchemaMismatch,
            format!("invalid deposit value `{value}` in listings.deposit"),
            "rebuild or migrate the listings database to match current schema",
        ));
    }

    let rounded = value.round();
    if (value - rounded).abs() > 1e-6 {
        return Err(LetError::new(
            ErrorCode::SchemaMismatch,
            format!(
                "non-integer deposit value `{value}` in listings.deposit; expected whole-number amount"
            ),
            "rebuild or migrate the listings database to match current schema",
        ));
    }

    if rounded < i64::MIN as f64 || rounded > i64::MAX as f64 {
        return Err(LetError::new(
            ErrorCode::SchemaMismatch,
            format!("deposit value `{value}` is out of supported range"),
            "rebuild or migrate the listings database to match current schema",
        ));
    }

    Ok(Some(rounded as i64))
}

fn table_exists(connection: &Connection, table_name: &str) -> Result<bool> {
    let count: i64 = connection.query_row(
        "SELECT COUNT(1) FROM sqlite_master WHERE type = 'table' AND name = ?1",
        params![table_name],
        |row| row.get(0),
    )?;

    Ok(count > 0)
}

#[derive(Debug)]
struct ListingRow {
    id: String,
    portal_rightmove: Option<String>,
    portal_zoopla: Option<String>,
    portal_onthemarket: Option<String>,
    uprn: Option<String>,
    uprn_source: Option<String>,
    uprn_confidence: Option<String>,
    url: String,
    address: String,
    postcode: String,
    region: Option<String>,
    lat: f64,
    lng: f64,
    pin_type: Option<String>,
    google_maps_url: String,
    google_maps_street_view_url: String,
    price: i64,
    price_display: String,
    bedrooms: i64,
    bathrooms: i64,
    property_type: String,
    description: String,
    floorplan_remote: Option<String>,
    floorplan_local: Option<String>,
    epc_remote: Option<String>,
    epc_local: Option<String>,
    map_satellite_remote: Option<String>,
    map_satellite_local: Option<String>,
    map_street_remote: Option<String>,
    map_street_local: Option<String>,
    epc_rating: Option<String>,
    floor_area_sqm: Option<f64>,
    epc_lodgement_date: Option<String>,
    epc_address_match: Option<i64>,
    epc_search_url: Option<String>,
    gigabit_availability: Option<f64>,
    listed_date: Option<String>,
    available_date: Option<String>,
    deposit: Option<f64>,
    agent_name: Option<String>,
    agent_phone: Option<String>,
    area_lsoa_code: Option<String>,
    area_lsoa_name: Option<String>,
    area_msoa_code: Option<String>,
    area_msoa_name: Option<String>,
    imd_rank: Option<i64>,
    imd_decile: Option<i64>,
    imd_score: Option<f64>,
    income_bhc: Option<f64>,
    income_ahc: Option<f64>,
    social_housing_pct: Option<f64>,
    population: Option<i64>,
    flood_risk_level: Option<String>,
    flood_risk_source: Option<String>,
    crime_count_12m: Option<i64>,
    crime_rate_per_1k: Option<f64>,
    crime_violent_12m: Option<i64>,
    crime_burglary_12m: Option<i64>,
    crime_robbery_12m: Option<i64>,
    crime_band: Option<String>,
    crime_trend: Option<String>,
    crime_updated_at: Option<String>,
    fetched_at: String,
    extraction_status: String,
    status: String,
    notion_page_id: Option<String>,
    assessed_at: Option<String>,
    assessed_score: Option<f64>,
}

impl ListingRow {
    fn from_row(row: &Row<'_>) -> rusqlite::Result<Self> {
        Ok(Self {
            id: row.get("id")?,
            portal_rightmove: row.get("portal_rightmove")?,
            portal_zoopla: row.get("portal_zoopla")?,
            portal_onthemarket: row.get("portal_onthemarket")?,
            uprn: row.get("uprn")?,
            uprn_source: row.get("uprn_source")?,
            uprn_confidence: row.get("uprn_confidence")?,
            url: row.get("url")?,
            address: row.get("address")?,
            postcode: row.get("postcode")?,
            region: row.get("region")?,
            lat: row.get("lat")?,
            lng: row.get("lng")?,
            pin_type: row.get("pin_type")?,
            google_maps_url: row.get("google_maps_url")?,
            google_maps_street_view_url: row.get("google_maps_street_view_url")?,
            price: row.get("price")?,
            price_display: row.get("price_display")?,
            bedrooms: row.get("bedrooms")?,
            bathrooms: row.get("bathrooms")?,
            property_type: row.get("property_type")?,
            description: row.get("description")?,
            floorplan_remote: row.get("floorplan_remote")?,
            floorplan_local: row.get("floorplan_local")?,
            epc_remote: row.get("epc_remote")?,
            epc_local: row.get("epc_local")?,
            map_satellite_remote: row.get("map_satellite_remote")?,
            map_satellite_local: row.get("map_satellite_local")?,
            map_street_remote: row.get("map_street_remote")?,
            map_street_local: row.get("map_street_local")?,
            epc_rating: row.get("epc_rating")?,
            floor_area_sqm: row.get("floor_area_sqm")?,
            epc_lodgement_date: row.get("epc_lodgement_date")?,
            epc_address_match: row.get("epc_address_match")?,
            epc_search_url: row.get("epc_search_url")?,
            gigabit_availability: row.get("gigabit_availability")?,
            listed_date: row.get("listed_date")?,
            available_date: row.get("available_date")?,
            deposit: row.get("deposit")?,
            agent_name: row.get("agent_name")?,
            agent_phone: row.get("agent_phone")?,
            area_lsoa_code: row.get("area_lsoa_code")?,
            area_lsoa_name: row.get("area_lsoa_name")?,
            area_msoa_code: row.get("area_msoa_code")?,
            area_msoa_name: row.get("area_msoa_name")?,
            imd_rank: row.get("imd_rank")?,
            imd_decile: row.get("imd_decile")?,
            imd_score: row.get("imd_score")?,
            income_bhc: row.get("income_bhc")?,
            income_ahc: row.get("income_ahc")?,
            social_housing_pct: row.get("social_housing_pct")?,
            population: row.get("population")?,
            flood_risk_level: row.get("flood_risk_level")?,
            flood_risk_source: row.get("flood_risk_source")?,
            crime_count_12m: row.get("crime_count_12m")?,
            crime_rate_per_1k: row.get("crime_rate_per_1k")?,
            crime_violent_12m: row.get("crime_violent_12m")?,
            crime_burglary_12m: row.get("crime_burglary_12m")?,
            crime_robbery_12m: row.get("crime_robbery_12m")?,
            crime_band: row.get("crime_band")?,
            crime_trend: row.get("crime_trend")?,
            crime_updated_at: row.get("crime_updated_at")?,
            fetched_at: row.get("fetched_at")?,
            extraction_status: row.get("extraction_status")?,
            status: row.get("status")?,
            notion_page_id: row.get("notion_page_id")?,
            assessed_at: row.get("assessed_at")?,
            assessed_score: row.get("assessed_score")?,
        })
    }
}

#[derive(Debug)]
struct ScoreRow {
    overall: f64,
    confidence: f64,
    affordability: f64,
    location: f64,
    liveability: f64,
    penalty_epc: f64,
    penalty_garden: f64,
    penalty_pets: f64,
    penalty_combined: f64,
    factor_monthly_rent: f64,
    factor_price_percentile: f64,
    factor_floor_area_sqm: Option<f64>,
    factor_floor_area_percentile: Option<f64>,
    factor_epc_band: Option<String>,
    factor_epc_numeric: Option<f64>,
    factor_true_monthly_cost: f64,
    factor_true_cost_percentile: f64,
    factor_station_miles: Option<f64>,
    factor_station_percentile: Option<f64>,
    factor_gigabit_pct: Option<f64>,
    factor_region_name: Option<String>,
    factor_priority_score: Option<f64>,
    factor_imd_decile: Option<i64>,
    factor_crime_rate_per_1k: Option<f64>,
    factor_crime_rate_percentile: Option<f64>,
    factor_garden_type: String,
    factor_heating_type: String,
    factor_pet_policy: String,
    factor_property_type: Option<String>,
    factor_bedrooms: i64,
}

impl ScoreRow {
    fn from_row(row: &Row<'_>) -> rusqlite::Result<Self> {
        Ok(Self {
            overall: row.get("overall")?,
            confidence: row.get("confidence")?,
            affordability: row.get("affordability")?,
            location: row.get("location")?,
            liveability: row.get("liveability")?,
            penalty_epc: row.get("penalty_epc")?,
            penalty_garden: row.get("penalty_garden")?,
            penalty_pets: row.get("penalty_pets")?,
            penalty_combined: row.get("penalty_combined")?,
            factor_monthly_rent: row.get("factor_monthly_rent")?,
            factor_price_percentile: row.get("factor_price_percentile")?,
            factor_floor_area_sqm: row.get("factor_floor_area_sqm")?,
            factor_floor_area_percentile: row.get("factor_floor_area_percentile")?,
            factor_epc_band: row.get("factor_epc_band")?,
            factor_epc_numeric: row.get("factor_epc_numeric")?,
            factor_true_monthly_cost: row.get("factor_true_monthly_cost")?,
            factor_true_cost_percentile: row.get("factor_true_cost_percentile")?,
            factor_station_miles: row.get("factor_station_miles")?,
            factor_station_percentile: row.get("factor_station_percentile")?,
            factor_gigabit_pct: row.get("factor_gigabit_pct")?,
            factor_region_name: row.get("factor_region_name")?,
            factor_priority_score: row.get("factor_priority_score")?,
            factor_imd_decile: row.get("factor_imd_decile")?,
            factor_crime_rate_per_1k: row.get("factor_crime_rate_per_1k")?,
            factor_crime_rate_percentile: row.get("factor_crime_rate_percentile")?,
            factor_garden_type: row.get("factor_garden_type")?,
            factor_heating_type: row.get("factor_heating_type")?,
            factor_pet_policy: row.get("factor_pet_policy")?,
            factor_property_type: row.get("factor_property_type")?,
            factor_bedrooms: row.get("factor_bedrooms")?,
        })
    }
}

#[derive(Debug)]
struct AssessmentRow {
    maintenance: String,
    light_and_space: String,
    photo_analysis: String,
    tradeoffs: Option<String>,
    neighborhood_analysis: Option<String>,
    recommendation: String,
    family_suitability: String,
    reasoning: String,
    score_adjustment: f64,
}

impl AssessmentRow {
    fn from_row(row: &Row<'_>) -> rusqlite::Result<Self> {
        Ok(Self {
            maintenance: row.get("maintenance")?,
            light_and_space: row.get("light_and_space")?,
            photo_analysis: row.get("photo_analysis")?,
            tradeoffs: row.get("tradeoffs")?,
            neighborhood_analysis: row.get("neighborhood_analysis")?,
            recommendation: row.get("recommendation")?,
            family_suitability: row.get("family_suitability")?,
            reasoning: row.get("reasoning")?,
            score_adjustment: row.get("score_adjustment")?,
        })
    }
}
