#![forbid(unsafe_code)]

pub mod config;
pub mod context;
pub mod db;
pub mod errors;
pub mod paths;
pub mod pipeline;
pub mod schema;
pub mod services;
pub mod sources;
pub mod utils;

pub use db::{
    DbMeta, close_listings_db, find_listing_by_id_from_db, load_listings_file, open_listings_db,
    update_listing_assessment, upsert_listings,
};
pub use errors::{ErrorCode, LetError, Result};
pub use pipeline::enrich::{EnrichmentMode, ListingEnrichmentReport, SourceEnricher};
pub use pipeline::score::{
    calculate_assessed_score, recalc_assessed_scores, score_listings_with_config,
};
