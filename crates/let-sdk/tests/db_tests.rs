#![forbid(unsafe_code)]

use let_sdk::db::{
    DbMeta, LISTINGS_SCHEMA_VERSION, close_listings_db, find_listing_by_id_from_db,
    list_known_portal_ids, load_listing_summaries, load_listings_file, load_listings_overview,
    open_listings_db, replace_listing_scores, replace_listings, update_listing_assessment,
    update_listing_notion_page_ids, upsert_listings,
};
use let_sdk::schema::listing::{
    Agent, AreaCodeName, AreaMetrics, CrimeBand, CrimeMetrics, CrimeTrend, EpcBand,
    ExtractionStatus, FloodRisk, GardenType, GeoLocation, HeatingType, ImdMetrics, IncomeMetrics,
    Lettings, Listing, ListingAssessment, ListingImage, ListingStatus, MaintenanceRating, MapViews,
    PetPolicy, PinType, PortalIds, Recommendation, RemoteLocalAsset, ScoreContext, ScoreFactors,
    ScorePenalties, ScorePercentiles, Scores, StationDistance, StatsSummary,
};
use tempfile::TempDir;

#[test]
fn open_listings_db_initializes_schema_and_pragmas() {
    let temp_dir = TempDir::new().expect("create tempdir");
    let db_path = temp_dir.path().join("let.db");

    let connection = open_listings_db(&db_path).expect("open sqlite db");

    let foreign_keys: i64 = connection
        .query_row("PRAGMA foreign_keys", [], |row| row.get(0))
        .expect("read pragma foreign_keys");
    assert_eq!(foreign_keys, 1);

    let busy_timeout: i64 = connection
        .query_row("PRAGMA busy_timeout", [], |row| row.get(0))
        .expect("read pragma busy_timeout");
    assert!(busy_timeout >= 5_000);

    let listings_table: i64 = connection
        .query_row(
            "SELECT COUNT(1) FROM sqlite_master WHERE type = 'table' AND name = 'listings'",
            [],
            |row| row.get(0),
        )
        .expect("check listings table");
    assert_eq!(listings_table, 1);

    let score_contexts_table: i64 = connection
        .query_row(
            "SELECT COUNT(1) FROM sqlite_master WHERE type = 'table' AND name = 'score_contexts'",
            [],
            |row| row.get(0),
        )
        .expect("check score_contexts table");
    assert_eq!(score_contexts_table, 1);

    let user_version: i32 = connection
        .pragma_query_value(None, "user_version", |row| row.get(0))
        .expect("read pragma user_version");
    assert_eq!(user_version, LISTINGS_SCHEMA_VERSION);

    close_listings_db(connection).expect("close sqlite db");
}

#[test]
fn upsert_and_roundtrip_listing_data() {
    let temp_dir = TempDir::new().expect("create tempdir");
    let db_path = temp_dir.path().join("let.db");

    let listing = sample_listing();
    let meta = DbMeta {
        updated_at: "2026-03-01T12:30:00.000Z".to_owned(),
        last_search_total: 1,
    };
    let search_urls = vec!["https://www.rightmove.co.uk/property-to-rent/find.html".to_owned()];
    let locations = vec!["London".to_owned()];

    upsert_listings(
        &db_path,
        std::slice::from_ref(&listing),
        &[],
        std::slice::from_ref(&listing),
        &meta,
        &search_urls,
        &locations,
    )
    .expect("upsert listings");

    let loaded = load_listings_file(&db_path).expect("load listings");
    assert_eq!(loaded.updated_at, meta.updated_at);
    assert_eq!(loaded.last_search_total, meta.last_search_total);
    assert_eq!(loaded.search_urls, search_urls);
    assert_eq!(loaded.locations, locations);
    assert_eq!(loaded.listings.len(), 1);

    let loaded_listing = &loaded.listings[0];
    assert_eq!(loaded_listing.id, listing.id);
    assert_eq!(
        loaded_listing.portal_ids.rightmove,
        listing.portal_ids.rightmove
    );
    assert_eq!(loaded_listing.price, listing.price);
    assert_eq!(loaded_listing.notes, listing.notes);
    assert_eq!(
        loaded_listing
            .scores
            .as_ref()
            .expect("scores are present")
            .overall,
        76.4
    );
    assert_eq!(
        loaded_listing
            .scores
            .as_ref()
            .expect("scores are present")
            .context
            .config_hash,
        "score-config-v1"
    );

    let overview = load_listings_overview(&db_path).expect("load overview");
    assert_eq!(overview.listing_count, 1);
    assert_eq!(overview.meta.updated_at, meta.updated_at);

    let summaries = load_listing_summaries(&db_path).expect("load summaries");
    assert_eq!(summaries.len(), 1);
    assert_eq!(summaries[0].portal_rightmove.as_deref(), Some("165432101"));
    assert_eq!(summaries[0].score, Some(76.4));
    assert!(summaries[0].has_assessment);
    assert_eq!(
        summaries[0].first_station_name.as_deref(),
        Some("St James's Park")
    );

    let known_ids = list_known_portal_ids(&db_path).expect("load portal ids");
    assert_eq!(known_ids, vec!["165432101".to_owned()]);

    let by_uuid = find_listing_by_id_from_db(&db_path, listing.id.as_str())
        .expect("find by uuid")
        .expect("listing by uuid exists");
    assert_eq!(by_uuid.id, listing.id);

    let by_portal = find_listing_by_id_from_db(
        &db_path,
        listing
            .portal_ids
            .rightmove
            .as_deref()
            .expect("rightmove id exists"),
    )
    .expect("find by portal id")
    .expect("listing by portal id exists");
    assert_eq!(by_portal.id, listing.id);

    let updated_assessment = ListingAssessment {
        maintenance: MaintenanceRating::Excellent,
        light_and_space: "Bright and airy".to_owned(),
        photo_analysis: "High quality photos with natural light".to_owned(),
        tradeoffs: Some("Smaller kitchen".to_owned()),
        neighborhood_analysis: Some("Quiet street with strong amenities".to_owned()),
        recommendation: Recommendation::StrongRecommend,
        family_suitability: let_sdk::schema::listing::FamilySuitability::Excellent,
        reasoning: "Best fit after reassessment".to_owned(),
        score_adjustment: 4.0,
    };

    update_listing_assessment(
        &db_path,
        listing.id.as_str(),
        &updated_assessment,
        80.4,
        "2026-03-01T14:00:00.000Z",
    )
    .expect("update assessment");

    let refreshed = find_listing_by_id_from_db(&db_path, listing.id.as_str())
        .expect("find listing after assessment update")
        .expect("listing remains available");

    assert_eq!(
        refreshed.assessed_at.as_deref(),
        Some("2026-03-01T14:00:00.000Z")
    );
    assert_eq!(refreshed.assessed_score, Some(80.4));
    assert_eq!(
        refreshed.assessment.expect("assessment exists"),
        updated_assessment
    );
}

#[test]
fn load_listings_fails_when_score_contexts_table_is_missing() {
    let temp_dir = TempDir::new().expect("create tempdir");
    let db_path = temp_dir.path().join("let.db");

    let connection = open_listings_db(&db_path).expect("open sqlite db");
    connection
        .execute_batch("DROP TABLE IF EXISTS score_contexts;")
        .expect("drop score_contexts table");
    close_listings_db(connection).expect("close sqlite db");

    let error = load_listings_file(&db_path).expect_err("expected schema mismatch");
    assert_eq!(error.code, let_sdk::ErrorCode::SchemaMismatch);
    assert!(
        error.message.contains("score_contexts"),
        "expected missing score_contexts table message, got: {}",
        error.message
    );
}

#[test]
fn load_listings_fails_when_score_context_row_is_missing_for_scored_listing() {
    let temp_dir = TempDir::new().expect("create tempdir");
    let db_path = temp_dir.path().join("let.db");

    let listing = sample_listing();
    let meta = DbMeta {
        updated_at: "2026-03-01T12:30:00.000Z".to_owned(),
        last_search_total: 1,
    };
    upsert_listings(
        &db_path,
        std::slice::from_ref(&listing),
        &[],
        std::slice::from_ref(&listing),
        &meta,
        &[],
        &[],
    )
    .expect("upsert listings");

    let connection = open_listings_db(&db_path).expect("open sqlite db");
    connection
        .execute(
            "DELETE FROM score_contexts WHERE listing_id = ?1",
            [&listing.id],
        )
        .expect("delete score context row");
    close_listings_db(connection).expect("close sqlite db");

    let error = load_listings_file(&db_path).expect_err("expected schema mismatch");
    assert_eq!(error.code, let_sdk::ErrorCode::SchemaMismatch);
    assert!(
        error.message.contains("missing score context row"),
        "expected missing score context row message, got: {}",
        error.message
    );
}

#[test]
fn load_listings_fails_when_schema_version_mismatches() {
    let temp_dir = TempDir::new().expect("create tempdir");
    let db_path = temp_dir.path().join("let.db");

    let connection = open_listings_db(&db_path).expect("open sqlite db");
    connection
        .pragma_update(None, "user_version", 999)
        .expect("set mismatched user_version");
    close_listings_db(connection).expect("close sqlite db");

    let error = load_listings_file(&db_path).expect_err("expected schema mismatch");
    assert_eq!(error.code, let_sdk::ErrorCode::SchemaMismatch);
    assert!(
        error.message.contains("schema version mismatch"),
        "expected version mismatch message, got: {}",
        error.message
    );
}

#[test]
fn targeted_listing_writes_update_graph_scores_and_notion_id() {
    let temp_dir = TempDir::new().expect("create tempdir");
    let db_path = temp_dir.path().join("let.db");

    let listing = sample_listing();
    let meta = DbMeta {
        updated_at: "2026-03-01T12:30:00.000Z".to_owned(),
        last_search_total: 1,
    };
    upsert_listings(
        &db_path,
        std::slice::from_ref(&listing),
        &[],
        std::slice::from_ref(&listing),
        &meta,
        &[],
        &[],
    )
    .expect("upsert listings");

    let mut patched_listing = listing.clone();
    patched_listing.address = "11 Updated Street, London".to_owned();
    patched_listing.notes = vec!["updated note".to_owned()];
    replace_listings(
        &db_path,
        std::slice::from_ref(&patched_listing),
        "2026-03-01T13:00:00.000Z",
    )
    .expect("replace listing graph");

    let mut rescored_listing = patched_listing.clone();
    let mut scores = rescored_listing.scores.clone().expect("scores");
    scores.overall = 81.0;
    scores.context.config_hash = "score-config-v2".to_owned();
    rescored_listing.scores = Some(scores);
    rescored_listing.assessed_score = Some(82.5);
    replace_listing_scores(
        &db_path,
        std::slice::from_ref(&rescored_listing),
        "2026-03-01T13:05:00.000Z",
    )
    .expect("replace listing scores");

    update_listing_notion_page_ids(
        &db_path,
        &[(rescored_listing.id.clone(), "notion-page-1".to_owned())],
        "2026-03-01T13:10:00.000Z",
    )
    .expect("update notion page id");

    let refreshed = find_listing_by_id_from_db(&db_path, rescored_listing.id.as_str())
        .expect("reload listing")
        .expect("listing exists");
    assert_eq!(refreshed.address, "11 Updated Street, London");
    assert_eq!(refreshed.notes, vec!["updated note".to_owned()]);
    assert_eq!(
        refreshed
            .scores
            .as_ref()
            .expect("scores are present")
            .overall,
        81.0
    );
    assert_eq!(refreshed.assessed_score, Some(82.5));
    assert_eq!(refreshed.notion_page_id.as_deref(), Some("notion-page-1"));
}

fn sample_listing() -> Listing {
    Listing {
        id: "2d8ab4a6-7de1-4e3f-a4aa-9408f2112377".to_owned(),
        portal_ids: PortalIds {
            rightmove: Some("165432101".to_owned()),
            zoopla: None,
            onthemarket: None,
        },
        uprn: Some("100021345678".to_owned()),
        uprn_source: Some(let_sdk::schema::listing::UprnSource::Epc),
        uprn_confidence: Some(let_sdk::schema::listing::UprnConfidence::Exact),
        url: "https://www.rightmove.co.uk/properties/165432101".to_owned(),
        location: GeoLocation {
            lat: 51.5074,
            lng: -0.1278,
            pin_type: Some(PinType::AccuratePoint),
        },
        postcode: "SW1A 1AA".to_owned(),
        address: "10 Example Street, London".to_owned(),
        region: Some("London".to_owned()),
        google_maps_url: "https://maps.google.com/?q=51.5074,-0.1278".to_owned(),
        google_maps_street_view_url: "https://maps.google.com/?layer=c&cbll=51.5074,-0.1278"
            .to_owned(),
        area: AreaMetrics {
            lsoa: AreaCodeName {
                code: Some("E01004736".to_owned()),
                name: Some("Westminster 018A".to_owned()),
            },
            msoa: AreaCodeName {
                code: Some("E02000977".to_owned()),
                name: Some("Westminster 018".to_owned()),
            },
            imd: ImdMetrics {
                rank: Some(20456),
                decile: Some(8),
                score: Some(8.42),
            },
            income: IncomeMetrics {
                bhc: Some(41_200.0),
                ahc: Some(35_900.0),
            },
            social_housing_pct: Some(12.0),
            population: Some(11_440),
            flood_risk: FloodRisk {
                level: Some("low".to_owned()),
                source: Some("ea".to_owned()),
            },
            crime: CrimeMetrics {
                count_12m: Some(380),
                rate_per_1k: Some(14.3),
                violent_12m: Some(44),
                burglary_12m: Some(12),
                robbery_12m: Some(6),
                band: Some(CrimeBand::Good),
                trend: Some(CrimeTrend::Stable),
                updated_at: Some("2026-01-31".to_owned()),
            },
        },
        price: 2_300,
        price_display: "£2,300 pcm".to_owned(),
        bedrooms: 2,
        bathrooms: 1,
        property_type: "Flat".to_owned(),
        description: "Spacious two-bedroom apartment in central London.".to_owned(),
        notes: vec![
            "balcony".to_owned(),
            "double-glazed windows".to_owned(),
            "near tube".to_owned(),
        ],
        images: vec![
            ListingImage {
                remote: "https://media.rightmove.co.uk/image-1.jpg".to_owned(),
                local: Some("/tmp/image-1.jpg".to_owned()),
            },
            ListingImage {
                remote: "https://media.rightmove.co.uk/image-2.jpg".to_owned(),
                local: None,
            },
        ],
        floorplan: RemoteLocalAsset {
            remote: Some("https://media.rightmove.co.uk/floorplan.jpg".to_owned()),
            local: Some("/tmp/floorplan.jpg".to_owned()),
        },
        epc: RemoteLocalAsset {
            remote: Some("https://epc.service.gov.uk/energy-certificate/abcd".to_owned()),
            local: None,
        },
        map_views: MapViews {
            satellite: RemoteLocalAsset {
                remote: Some("https://maps.example.com/satellite.png".to_owned()),
                local: Some("/tmp/satellite.png".to_owned()),
            },
            street: RemoteLocalAsset {
                remote: Some("https://maps.example.com/street.png".to_owned()),
                local: Some("/tmp/street.png".to_owned()),
            },
        },
        epc_rating: Some(EpcBand::C),
        floor_area_sqm: Some(72.5),
        epc_lodgement_date: Some("2025-06-11".to_owned()),
        epc_address_match: Some(true),
        epc_search_url: Some(
            "https://epc.service.gov.uk/find-a-certificate/search-by-postcode".to_owned(),
        ),
        nearest_stations: vec![
            StationDistance {
                name: "St James's Park".to_owned(),
                distance: 0.3,
                unit: "miles".to_owned(),
            },
            StationDistance {
                name: "Victoria".to_owned(),
                distance: 0.6,
                unit: "miles".to_owned(),
            },
        ],
        gigabit_availability: Some(0.98),
        listed_date: Some("2026-02-20".to_owned()),
        lettings: Lettings {
            available_date: Some("2026-03-10".to_owned()),
            deposit: Some(2_650),
        },
        agent: Agent {
            name: Some("Example Lettings".to_owned()),
            phone: Some("02070000000".to_owned()),
        },
        assessment: Some(ListingAssessment {
            maintenance: MaintenanceRating::Good,
            light_and_space: "Good natural light".to_owned(),
            photo_analysis: "Rooms appear well maintained".to_owned(),
            tradeoffs: Some("Limited storage".to_owned()),
            neighborhood_analysis: Some("Close to schools and green space".to_owned()),
            recommendation: Recommendation::Recommend,
            family_suitability: let_sdk::schema::listing::FamilySuitability::Good,
            reasoning: "Solid overall option with minor tradeoffs".to_owned(),
            score_adjustment: 1.5,
        }),
        assessed_at: Some("2026-03-01T13:00:00.000Z".to_owned()),
        assessed_score: Some(77.9),
        scores: Some(sample_scores()),
        fetched_at: "2026-03-01T12:00:00.000Z".to_owned(),
        extraction_status: ExtractionStatus::Success,
        status: ListingStatus::Active,
        notion_page_id: None,
    }
}

fn sample_scores() -> Scores {
    Scores {
        overall: 76.4,
        confidence: 0.91,
        affordability: 74.3,
        location: 79.5,
        liveability: 75.0,
        factors: ScoreFactors {
            monthly_rent: 2_300.0,
            price_percentile: 0.62,
            floor_area_sqm: Some(72.5),
            floor_area_percentile: Some(0.57),
            epc_band: Some("C".to_owned()),
            epc_numeric: Some(3.0),
            true_monthly_cost: 2_450.0,
            true_cost_percentile: 0.60,
            station_miles: Some(0.3),
            station_percentile: Some(0.32),
            gigabit_pct: Some(0.98),
            region_name: Some("London".to_owned()),
            priority_score: Some(0.88),
            garden_type: GardenType::Shared,
            heating_type: HeatingType::Gas,
            pet_policy: PetPolicy::Yes,
            property_type: Some("Flat".to_owned()),
            bedrooms: 2,
            imd_decile: Some(8),
            crime_rate_per_1k: Some(14.3),
            crime_rate_percentile: Some(0.30),
        },
        penalties: ScorePenalties {
            epc: 0.0,
            garden: -0.5,
            pets: 0.0,
            combined: -0.5,
        },
        context: ScoreContext {
            config_hash: "score-config-v1".to_owned(),
            percentiles: ScorePercentiles {
                prices: stats(1_200.0, 3_200.0, 2_100.0, 2_050.0, 420.0),
                true_costs: stats(1_300.0, 3_450.0, 2_240.0, 2_180.0, 440.0),
                floor_areas: stats(36.0, 140.0, 72.0, 70.0, 18.0),
                station_distances: stats(0.1, 2.6, 0.8, 0.6, 0.5),
                crime_rates: stats(4.0, 40.0, 18.0, 16.0, 7.0),
            },
        },
    }
}

fn stats(min: f64, max: f64, mean: f64, median: f64, std_dev: f64) -> StatsSummary {
    StatsSummary {
        min,
        max,
        mean,
        median,
        std_dev,
    }
}
