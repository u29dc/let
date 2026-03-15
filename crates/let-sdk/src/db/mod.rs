#![forbid(unsafe_code)]

use std::fs;
use std::path::Path;
use std::time::Duration;

use rusqlite::{Connection, OpenFlags};

use crate::{ErrorCode, LetError, Result};

mod repository;

pub use repository::{
    DbMeta, ListingSummary, ListingsOverview, find_listing_by_id_from_db, list_known_portal_ids,
    load_listing_summaries, load_listings_file, load_listings_overview, replace_listing_scores,
    replace_listings, update_listing_assessment, update_listing_notion_page_ids, upsert_listings,
};

const LISTINGS_SCHEMA_SQL: &str = include_str!("schema.sql");
const SQLITE_BUSY_TIMEOUT: Duration = Duration::from_millis(5_000);
pub const LISTINGS_SCHEMA_VERSION: i32 = 2;

pub fn open_listings_db(path: impl AsRef<Path>) -> Result<Connection> {
    let path = path.as_ref();
    let existing_database = path.exists()
        && fs::metadata(path)
            .map(|metadata| metadata.len() > 0)
            .unwrap_or(false);
    if let Some(parent) = path.parent()
        && !parent.as_os_str().is_empty()
    {
        fs::create_dir_all(parent)?;
    }

    let connection = Connection::open(path)?;
    connection.pragma_update(None, "foreign_keys", "ON")?;
    connection.busy_timeout(SQLITE_BUSY_TIMEOUT)?;
    if existing_database {
        ensure_schema_version(&connection)?;
    }
    init_schema(&connection)?;
    set_schema_version(&connection)?;

    Ok(connection)
}

pub fn open_listings_db_readonly(path: impl AsRef<Path>) -> Result<Connection> {
    let path = path.as_ref();
    if !path.exists() {
        return Err(LetError::new(
            ErrorCode::NotFound,
            format!("listings database not found at {}", path.display()),
            "run `let fetch <id>` to create and populate the listings database",
        ));
    }

    let connection = Connection::open_with_flags(path, OpenFlags::SQLITE_OPEN_READ_ONLY)?;
    connection.pragma_update(None, "foreign_keys", "ON")?;
    connection.busy_timeout(SQLITE_BUSY_TIMEOUT)?;
    ensure_schema_version(&connection)?;

    Ok(connection)
}

pub fn close_listings_db(connection: Connection) -> Result<()> {
    connection
        .close()
        .map_err(|(_connection, error)| LetError::from(error))?;
    Ok(())
}

pub fn init_schema(connection: &Connection) -> Result<()> {
    connection.execute_batch(LISTINGS_SCHEMA_SQL)?;
    Ok(())
}

fn set_schema_version(connection: &Connection) -> Result<()> {
    connection.pragma_update(None, "user_version", LISTINGS_SCHEMA_VERSION)?;
    Ok(())
}

fn ensure_schema_version(connection: &Connection) -> Result<()> {
    let version: i32 = connection.pragma_query_value(None, "user_version", |row| row.get(0))?;
    if version == LISTINGS_SCHEMA_VERSION {
        return Ok(());
    }

    Err(LetError::new(
        ErrorCode::SchemaMismatch,
        format!(
            "listings database schema version mismatch: expected {LISTINGS_SCHEMA_VERSION}, found {version}"
        ),
        "delete the listings database and rerun `let fetch <id>` to recreate it",
    ))
}
