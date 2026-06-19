-- let.db Schema
-- Schema version: 2
-- Last updated: 2026-03-15
--
-- Complete DDL for the listings database.
-- Tables ordered by dependency (parent tables first).
--
-- Usage:
--   sqlite3 .let/data/let.db < crates/let-sdk/src/db/schema.sql
--
-- To inspect current schema:
--   sqlite3 .let/data/let.db .schema

--------------------------------------------------------------------------------
-- listings: Root table for property listings
--------------------------------------------------------------------------------
-- Stores core property data from Rightmove including location, price,
-- property details, EPC data, broadband availability, and sync status.

CREATE TABLE IF NOT EXISTS listings (
    id TEXT PRIMARY KEY,
    portal_rightmove TEXT NULL,
    portal_zoopla TEXT NULL,
    portal_onthemarket TEXT NULL,
    uprn TEXT NULL,
    uprn_source TEXT NULL,
    uprn_confidence TEXT NULL,
    url TEXT NOT NULL,
    address TEXT NOT NULL,
    postcode TEXT NOT NULL,
    region TEXT NULL,
    lat REAL NOT NULL,
    lng REAL NOT NULL,
    pin_type TEXT NULL,
    google_maps_url TEXT NOT NULL,
    google_maps_street_view_url TEXT NOT NULL,
    price INTEGER NOT NULL,
    price_display TEXT NOT NULL,
    bedrooms INTEGER NOT NULL,
    bathrooms INTEGER NOT NULL,
    property_type TEXT NOT NULL,
    description TEXT NOT NULL,
    floorplan_remote TEXT NULL,
    floorplan_local TEXT NULL,
    epc_remote TEXT NULL,
    epc_local TEXT NULL,
    map_satellite_remote TEXT NULL,
    map_satellite_local TEXT NULL,
    map_street_remote TEXT NULL,
    map_street_local TEXT NULL,
    epc_rating TEXT NULL,
    floor_area_sqm REAL NULL,
    epc_lodgement_date TEXT NULL,
    epc_address_match INTEGER NULL,
    epc_search_url TEXT NULL,
    gigabit_availability REAL NULL,
    listed_date TEXT NULL,
    available_date TEXT NULL,
    deposit INTEGER NULL,
    agent_name TEXT NULL,
    agent_phone TEXT NULL,
    area_lsoa_code TEXT NULL,
    area_lsoa_name TEXT NULL,
    area_msoa_code TEXT NULL,
    area_msoa_name TEXT NULL,
    imd_rank INTEGER NULL,
    imd_decile INTEGER NULL,
    imd_score REAL NULL,
    income_bhc REAL NULL,
    income_ahc REAL NULL,
    social_housing_pct REAL NULL,
    population INTEGER NULL,
    flood_risk_level TEXT NULL,
    flood_risk_source TEXT NULL,
    crime_count_12m INTEGER NULL,
    crime_rate_per_1k REAL NULL,
    crime_violent_12m INTEGER NULL,
    crime_burglary_12m INTEGER NULL,
    crime_robbery_12m INTEGER NULL,
    crime_band TEXT NULL,
    crime_trend TEXT NULL,
    crime_updated_at TEXT NULL,
    fetched_at TEXT NOT NULL,
    extraction_status TEXT NOT NULL,
    status TEXT NOT NULL,
    notion_page_id TEXT NULL,
    assessed_at TEXT NULL,
    assessed_score REAL NULL
);

--------------------------------------------------------------------------------
-- stations: Nearest train/tube stations for each listing
--------------------------------------------------------------------------------
-- Ordered array of stations with distance. Position maintains order from
-- Rightmove (closest first).

CREATE TABLE IF NOT EXISTS stations (
    listing_id TEXT NOT NULL,
    name TEXT NOT NULL,
    distance REAL NOT NULL,
    unit TEXT NOT NULL,
    position INTEGER NOT NULL,
    PRIMARY KEY (listing_id, position),
    FOREIGN KEY (listing_id) REFERENCES listings(id) ON DELETE CASCADE
);

--------------------------------------------------------------------------------
-- images: Property photographs for each listing
--------------------------------------------------------------------------------
-- Ordered array of images. Remote URL from Rightmove CDN, local path for
-- cached images. Position maintains gallery order.

CREATE TABLE IF NOT EXISTS images (
    listing_id TEXT NOT NULL,
    remote TEXT NOT NULL,
    local TEXT NULL,
    position INTEGER NOT NULL,
    PRIMARY KEY (listing_id, position),
    FOREIGN KEY (listing_id) REFERENCES listings(id) ON DELETE CASCADE
);

--------------------------------------------------------------------------------
-- notes: Extracted property notes/features
--------------------------------------------------------------------------------
-- Pattern-matched features from description (garden, parking, condition, etc).
-- Position maintains extraction order.

CREATE TABLE IF NOT EXISTS notes (
    listing_id TEXT NOT NULL,
    note TEXT NOT NULL,
    position INTEGER NOT NULL,
    PRIMARY KEY (listing_id, position),
    FOREIGN KEY (listing_id) REFERENCES listings(id) ON DELETE CASCADE
);

--------------------------------------------------------------------------------
-- scores: Calculated scores for each listing
--------------------------------------------------------------------------------
-- Scoring algorithm output including composite scores (affordability, location,
-- liveability), penalties, and all input factors used in calculation.

CREATE TABLE IF NOT EXISTS scores (
    listing_id TEXT PRIMARY KEY,
    overall REAL NOT NULL,
    confidence REAL NOT NULL,
    affordability REAL NOT NULL,
    location REAL NOT NULL,
    liveability REAL NOT NULL,
    penalty_epc REAL NOT NULL,
    penalty_garden REAL NOT NULL,
    penalty_pets REAL NOT NULL,
    penalty_combined REAL NOT NULL,
    factor_monthly_rent REAL NOT NULL,
    factor_price_percentile REAL NOT NULL,
    factor_floor_area_sqm REAL NULL,
    factor_floor_area_percentile REAL NULL,
    factor_epc_band TEXT NULL,
    factor_epc_numeric REAL NULL,
    factor_true_monthly_cost REAL NOT NULL,
    factor_true_cost_percentile REAL NOT NULL,
    factor_station_miles REAL NULL,
    factor_station_percentile REAL NULL,
    factor_gigabit_pct REAL NULL,
    factor_region_name TEXT NULL,
    factor_priority_score REAL NULL,
    factor_imd_decile INTEGER NULL,
    factor_crime_rate_per_1k REAL NULL,
    factor_crime_rate_percentile REAL NULL,
    factor_garden_type TEXT NOT NULL,
    factor_heating_type TEXT NOT NULL,
    factor_pet_policy TEXT NOT NULL,
    factor_property_type TEXT NULL,
    factor_bedrooms INTEGER NOT NULL,
    FOREIGN KEY (listing_id) REFERENCES listings(id) ON DELETE CASCADE
);

--------------------------------------------------------------------------------
-- score_contexts: Scoring context metadata for each listing score
--------------------------------------------------------------------------------
-- Persists score config hash and percentile stats used when computing scores.

CREATE TABLE IF NOT EXISTS score_contexts (
    listing_id TEXT PRIMARY KEY,
    context_json TEXT NOT NULL,
    FOREIGN KEY (listing_id) REFERENCES listings(id) ON DELETE CASCADE
);

--------------------------------------------------------------------------------
-- assessments: AI-assisted property assessments
--------------------------------------------------------------------------------
-- Structured evaluation from CLI assess command. Captures maintenance quality,
-- light/space, recommendations, and score adjustments.

CREATE TABLE IF NOT EXISTS assessments (
    listing_id TEXT PRIMARY KEY,
    maintenance TEXT NOT NULL,
    light_and_space TEXT NOT NULL,
    photo_analysis TEXT NOT NULL,
    tradeoffs TEXT NULL,
    neighborhood_analysis TEXT NULL,
    recommendation TEXT NOT NULL,
    family_suitability TEXT NOT NULL,
    reasoning TEXT NOT NULL,
    score_adjustment REAL NOT NULL,
    FOREIGN KEY (listing_id) REFERENCES listings(id) ON DELETE CASCADE
);

--------------------------------------------------------------------------------
-- search_urls: Rightmove search URLs used for fetching
--------------------------------------------------------------------------------
-- Tracks which search URLs have been processed. Used for incremental updates.

CREATE TABLE IF NOT EXISTS search_urls (
    url TEXT PRIMARY KEY
);

--------------------------------------------------------------------------------
-- search_locations: Search location names
--------------------------------------------------------------------------------
-- Region names used in searches (e.g., "Manchester, Greater Manchester").

CREATE TABLE IF NOT EXISTS search_locations (
    name TEXT PRIMARY KEY
);

--------------------------------------------------------------------------------
-- meta: Database metadata
--------------------------------------------------------------------------------
-- Single-row table for database-level metadata. Constrained to exactly one row.

CREATE TABLE IF NOT EXISTS meta (
    id INTEGER PRIMARY KEY CHECK (id = 1),
    updated_at TEXT NOT NULL,
    last_search_total INTEGER NOT NULL
);

--------------------------------------------------------------------------------
-- INDEXES: Secondary indexes for common query patterns
--------------------------------------------------------------------------------

-- Region filtering in legacy listing repository tests.
CREATE INDEX IF NOT EXISTS idx_listings_region ON listings(region);

-- Status filtering in legacy listing repository tests.
CREATE INDEX IF NOT EXISTS idx_listings_status ON listings(status);

-- Combined region + status: common filter pattern
CREATE INDEX IF NOT EXISTS idx_listings_status_region ON listings(status, region);

-- Rightmove ID lookup
CREATE UNIQUE INDEX IF NOT EXISTS idx_listings_portal_rightmove ON listings(portal_rightmove) WHERE portal_rightmove IS NOT NULL;

-- Score sorting in legacy listing repository tests.
CREATE INDEX IF NOT EXISTS idx_scores_overall ON scores(overall DESC);
