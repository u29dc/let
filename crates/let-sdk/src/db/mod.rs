#![forbid(unsafe_code)]

use std::fs;
use std::path::Path;
use std::time::Duration;

use rusqlite::Connection;

use crate::{LetError, Result};

mod repository;

pub use repository::{
    DbMeta, find_listing_by_id_from_db, load_listings_file, update_listing_assessment,
    upsert_listings,
};

const LISTINGS_SCHEMA_SQL: &str = include_str!("schema.sql");
const SQLITE_BUSY_TIMEOUT: Duration = Duration::from_millis(5_000);

pub fn open_listings_db(path: impl AsRef<Path>) -> Result<Connection> {
    let path = path.as_ref();
    if let Some(parent) = path.parent()
        && !parent.as_os_str().is_empty()
    {
        fs::create_dir_all(parent)?;
    }

    let connection = Connection::open(path)?;
    connection.pragma_update(None, "foreign_keys", "ON")?;
    connection.busy_timeout(SQLITE_BUSY_TIMEOUT)?;
    init_schema(&connection)?;

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
