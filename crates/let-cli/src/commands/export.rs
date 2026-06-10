#![forbid(unsafe_code)]

use std::cmp::Ordering;
use std::fs;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use let_sdk::schema::listing::{Listing, ListingStatus};
use let_sdk::{load_listings_file, update_listing_notion_page_ids};
use reqwest::header::{AUTHORIZATION, CONTENT_TYPE, HeaderMap, HeaderValue};
use serde::Serialize;
use serde_json::{Value, json};

use crate::commands::{CommandError, CommandOutput, CommandResult, SharedArgs, to_camel_json};
use crate::env::resolve_env_var;

const NOTION_API_BASE: &str = "https://api.notion.com/v1";
const NOTION_API_VERSION: &str = "2022-06-28";
const NOTION_MIN_DELAY_MS: u64 = 350;
const NOTION_MAX_RETRIES: usize = 3;

#[derive(Debug, Clone)]
pub struct NotionParams {
    pub top: Option<usize>,
    pub min_score: Option<f64>,
    pub region: Option<String>,
    pub dry_run: bool,
    pub force: bool,
}

#[derive(Debug, Clone)]
struct NotionConfig {
    api_key: String,
    database_id: String,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct NotionExportOutput {
    created: usize,
    updated: usize,
    skipped: usize,
    failed: usize,
    total: usize,
    dry_run: bool,
}

pub fn export_json(shared: &SharedArgs, output: Option<PathBuf>) -> CommandResult {
    let paths = let_sdk::paths::resolve_paths(Some(shared.overrides.clone()));
    let db_path = paths.derived.database;
    let output_path = output.unwrap_or(paths.derived.json_export);

    let data = load_listings_file(&db_path)?;
    let payload = to_camel_json(&data);
    let pretty = serde_json::to_string_pretty(&payload).expect("json serialization should succeed");

    if let Some(parent) = output_path.parent() {
        fs::create_dir_all(parent).map_err(|error| {
            CommandError::runtime(
                "IO_ERROR",
                format!("failed to create export directory: {error}"),
                "check filesystem permissions for output path",
            )
        })?;
    }
    fs::write(&output_path, pretty).map_err(|error| {
        CommandError::runtime(
            "IO_ERROR",
            format!("failed to write export file: {error}"),
            "check filesystem permissions and free space",
        )
    })?;

    Ok(CommandOutput::new(json!({
        "path": output_path.display().to_string(),
        "count": data.listings.len(),
    }))
    .with_count(data.listings.len()))
}

pub fn export_notion(shared: &SharedArgs, params: &NotionParams) -> CommandResult {
    let paths = let_sdk::paths::resolve_paths(Some(shared.overrides.clone()));
    let notion = load_notion_config(&paths.derived.env_file)?;
    let runtime = build_runtime()?;
    let mut client = NotionClient::new(&runtime, notion)?;
    client.validate_database()?;

    let db_path = paths.derived.database;
    let mut data = load_listings_file(&db_path)?;

    if data.listings.is_empty() {
        let payload = NotionExportOutput {
            created: 0,
            updated: 0,
            skipped: 0,
            failed: 0,
            total: 0,
            dry_run: params.dry_run,
        };
        return Ok(CommandOutput::new(to_camel_json(&payload)));
    }

    let selected = select_listing_indices(&data.listings, params);
    if params.dry_run {
        let payload = NotionExportOutput {
            created: 0,
            updated: 0,
            skipped: 0,
            failed: 0,
            total: selected.len(),
            dry_run: true,
        };
        return Ok(CommandOutput::new(to_camel_json(&payload))
            .with_count(selected.len())
            .with_total(selected.len())
            .with_has_more(false));
    }

    let mut created = 0usize;
    let mut updated = 0usize;
    let mut skipped = 0usize;
    let mut failed = 0usize;
    let mut page_updates = Vec::<(String, String)>::new();

    for index in selected.iter().copied() {
        let Some(listing) = data.listings.get_mut(index) else {
            continue;
        };
        if listing.notion_page_id.is_some() && !params.force {
            skipped += 1;
            continue;
        }

        let had_page = listing.notion_page_id.is_some();
        let result = if let Some(page_id) = listing.notion_page_id.clone() {
            client.update_page(&page_id, listing)
        } else {
            client.create_page(listing).map(|page_id| {
                listing.notion_page_id = Some(page_id);
            })
        };

        match result {
            Ok(()) => {
                if had_page {
                    updated += 1;
                } else {
                    created += 1;
                    if let Some(page_id) = listing.notion_page_id.clone() {
                        page_updates.push((listing.id.clone(), page_id));
                    }
                }
            }
            Err(_) => {
                failed += 1;
            }
        }
    }

    update_listing_notion_page_ids(&db_path, &page_updates, &let_sdk::utils::time::now_iso())?;

    let payload = NotionExportOutput {
        created,
        updated,
        skipped,
        failed,
        total: selected.len(),
        dry_run: false,
    };

    Ok(CommandOutput::new(to_camel_json(&payload))
        .with_count(payload.total)
        .with_total(payload.total)
        .with_has_more(false))
}

fn build_runtime() -> Result<tokio::runtime::Runtime, CommandError> {
    tokio::runtime::Builder::new_multi_thread()
        .worker_threads(1)
        .enable_all()
        .build()
        .map_err(|error| {
            CommandError::runtime(
                "PROCESS_ERROR",
                format!("failed to initialize runtime: {error}"),
                "retry command",
            )
        })
}

fn load_notion_config(env_file: &Path) -> Result<NotionConfig, CommandError> {
    let api_key = resolve_env_var("NOTION_API_KEY", env_file)
        .map(|(value, _)| value)
        .ok_or_else(|| {
            CommandError::new(
                "NO_CREDENTIALS",
                "missing NOTION_API_KEY",
                "set NOTION_API_KEY and NOTION_DATABASE_ID",
                2,
            )
        })?;
    let database_id = resolve_env_var("NOTION_DATABASE_ID", env_file)
        .map(|(value, _)| value)
        .ok_or_else(|| {
            CommandError::new(
                "NO_CREDENTIALS",
                "missing NOTION_DATABASE_ID",
                "set NOTION_API_KEY and NOTION_DATABASE_ID",
                2,
            )
        })?;

    Ok(NotionConfig {
        api_key,
        database_id,
    })
}

fn select_listing_indices(listings: &[Listing], params: &NotionParams) -> Vec<usize> {
    let mut selected = listings
        .iter()
        .enumerate()
        .filter(|(_, listing)| matches!(listing.status, ListingStatus::Active))
        .filter(|(_, listing)| {
            params.min_score.is_none_or(|threshold| {
                listing
                    .scores
                    .as_ref()
                    .is_some_and(|scores| scores.overall >= threshold)
            })
        })
        .filter(|(_, listing)| {
            params.region.as_ref().is_none_or(|region| {
                let target = region.to_ascii_lowercase();
                listing
                    .region
                    .as_deref()
                    .is_some_and(|value| value.to_ascii_lowercase().contains(&target))
            })
        })
        .collect::<Vec<_>>();

    selected.sort_by(|(_, left), (_, right)| {
        let left_score = left
            .assessed_score
            .or_else(|| left.scores.as_ref().map(|scores| scores.overall))
            .unwrap_or(0.0);
        let right_score = right
            .assessed_score
            .or_else(|| right.scores.as_ref().map(|scores| scores.overall))
            .unwrap_or(0.0);
        right_score
            .partial_cmp(&left_score)
            .unwrap_or(Ordering::Equal)
    });

    if let Some(top) = params.top
        && selected.len() > top
    {
        selected.truncate(top);
    }

    selected.into_iter().map(|(index, _)| index).collect()
}

struct NotionClient<'a> {
    runtime: &'a tokio::runtime::Runtime,
    http: reqwest::Client,
    config: NotionConfig,
    last_request_at: Option<Instant>,
}

impl<'a> NotionClient<'a> {
    fn new(
        runtime: &'a tokio::runtime::Runtime,
        config: NotionConfig,
    ) -> Result<Self, CommandError> {
        let http = runtime
            .block_on(async {
                reqwest::Client::builder()
                    .timeout(Duration::from_secs(30))
                    .build()
            })
            .map_err(|error| {
                CommandError::runtime(
                    "NETWORK_ERROR",
                    format!("failed to build notion client: {error}"),
                    "check TLS/certificate configuration",
                )
            })?;

        Ok(Self {
            runtime,
            http,
            config,
            last_request_at: None,
        })
    }

    fn validate_database(&mut self) -> Result<(), CommandError> {
        let path = format!("/databases/{}", self.config.database_id);
        self.request_json(reqwest::Method::GET, &path, None)
            .map(|_| ())
            .map_err(|error| {
                CommandError::new(
                    "INVALID_DB",
                    format!("cannot access notion database: {}", error.message),
                    "check NOTION_API_KEY and NOTION_DATABASE_ID",
                    2,
                )
            })
    }

    fn create_page(&mut self, listing: &Listing) -> Result<String, CommandError> {
        let body = json!({
            "parent": { "database_id": self.config.database_id },
            "properties": build_notion_properties(listing),
        });

        let response = self.request_json(reqwest::Method::POST, "/pages", Some(body))?;
        let page_id = response
            .get("id")
            .and_then(Value::as_str)
            .ok_or_else(|| {
                CommandError::runtime(
                    "EXPORT_ERROR",
                    "notion page create response missing id",
                    "retry export command",
                )
            })?
            .to_owned();
        Ok(page_id)
    }

    fn update_page(&mut self, page_id: &str, listing: &Listing) -> Result<(), CommandError> {
        let body = json!({
            "properties": build_notion_properties(listing),
        });
        self.request_json(
            reqwest::Method::PATCH,
            &format!("/pages/{page_id}"),
            Some(body),
        )?;
        Ok(())
    }

    fn request_json(
        &mut self,
        method: reqwest::Method,
        path: &str,
        body: Option<Value>,
    ) -> Result<Value, CommandError> {
        let mut last_error = String::new();
        let url = format!("{NOTION_API_BASE}{path}");

        for attempt in 1..=NOTION_MAX_RETRIES {
            self.apply_rate_limit_delay();

            let response = self.runtime.block_on(async {
                let mut headers = HeaderMap::new();
                let auth = format!("Bearer {}", self.config.api_key);
                headers.insert(
                    AUTHORIZATION,
                    HeaderValue::from_str(&auth).unwrap_or(HeaderValue::from_static("")),
                );
                headers.insert(
                    "Notion-Version",
                    HeaderValue::from_static(NOTION_API_VERSION),
                );
                headers.insert(CONTENT_TYPE, HeaderValue::from_static("application/json"));

                let mut request = self.http.request(method.clone(), &url).headers(headers);
                if let Some(payload) = body.clone() {
                    request = request.body(payload.to_string());
                }
                request.send().await
            });

            match response {
                Ok(response) if response.status().is_success() => {
                    return self
                        .runtime
                        .block_on(async { response.json::<Value>().await })
                        .map_err(|error| {
                            CommandError::runtime(
                                "PARSE_ERROR",
                                format!("failed to decode notion response: {error}"),
                                "retry export command",
                            )
                        });
                }
                Ok(response) => {
                    let status = response.status().as_u16();
                    let retryable = status == 429 || status >= 500;
                    let body_text = self
                        .runtime
                        .block_on(async { response.text().await })
                        .unwrap_or_default();
                    last_error = if body_text.is_empty() {
                        format!("http {status}")
                    } else {
                        format!("http {status}: {body_text}")
                    };

                    if retryable && attempt < NOTION_MAX_RETRIES {
                        std::thread::sleep(Duration::from_millis((attempt as u64) * 1000));
                        continue;
                    }
                    return Err(CommandError::runtime(
                        "EXPORT_ERROR",
                        format!("notion request failed: {last_error}"),
                        "check notion credentials, schema, and rate limits",
                    ));
                }
                Err(error) => {
                    last_error = error.to_string();
                    if attempt < NOTION_MAX_RETRIES {
                        std::thread::sleep(Duration::from_millis((attempt as u64) * 1000));
                        continue;
                    }
                    return Err(CommandError::runtime(
                        "NETWORK_ERROR",
                        format!("notion request failed: {last_error}"),
                        "check network connectivity and retry",
                    ));
                }
            }
        }

        Err(CommandError::runtime(
            "EXPORT_ERROR",
            format!("notion request failed: {last_error}"),
            "retry export command",
        ))
    }

    fn apply_rate_limit_delay(&mut self) {
        if let Some(last_request_at) = self.last_request_at {
            let elapsed = last_request_at.elapsed();
            if elapsed < Duration::from_millis(NOTION_MIN_DELAY_MS) {
                std::thread::sleep(Duration::from_millis(NOTION_MIN_DELAY_MS) - elapsed);
            }
        }
        self.last_request_at = Some(Instant::now());
    }
}

fn build_notion_properties(listing: &Listing) -> Value {
    let score = listing
        .assessed_score
        .or_else(|| listing.scores.as_ref().map(|scores| scores.overall));
    let garden = listing
        .scores
        .as_ref()
        .and_then(|scores| enum_to_string(&scores.factors.garden_type));
    let heating = listing
        .scores
        .as_ref()
        .and_then(|scores| enum_to_string(&scores.factors.heating_type));
    let pets = listing
        .scores
        .as_ref()
        .and_then(|scores| enum_to_string(&scores.factors.pet_policy));
    let epc = listing.epc_rating.as_ref().and_then(enum_to_string);

    let mut image_urls = listing
        .images
        .iter()
        .map(|image| image.remote.clone())
        .collect::<Vec<_>>();
    if let Some(satellite) = listing.map_views.satellite.remote.as_ref() {
        image_urls.insert(0, satellite.clone());
    }

    json!({
        "Name": notion_title(&listing.address),
        "Price": notion_number(Some(listing.price as f64)),
        "Bedrooms": notion_number(Some(listing.bedrooms as f64)),
        "Bathrooms": notion_number(Some(listing.bathrooms as f64)),
        "Floor Area": notion_number(listing.floor_area_sqm),
        "Score": notion_number(score),
        "EPC": notion_select(epc.as_deref()),
        "Garden": notion_select(garden.as_deref()),
        "Heating": notion_select(heating.as_deref()),
        "Pets": notion_select(pets.as_deref()),
        "Type": notion_rich_text(Some(&listing.property_type)),
        "Region": notion_rich_text(listing.region.as_deref()),
        "Notes": notion_rich_text(Some(&listing.notes.join("\n"))),
        "Address Text": notion_rich_text(Some(&format!(
            "{}, {} [{},{}]",
            listing.address, listing.postcode, listing.location.lat, listing.location.lng
        ))),
        "URL": notion_url(Some(&listing.url)),
        "Google Maps": notion_url(Some(&listing.google_maps_url)),
        "Google Street View": notion_url(Some(&listing.google_maps_street_view_url)),
        "Notes (AI)": notion_rich_text(listing.assessment.as_ref().map(|assessment| assessment.reasoning.as_str())),
        "Images": notion_files(&image_urls),
    })
}

fn enum_to_string<T: Serialize>(value: &T) -> Option<String> {
    serde_json::to_value(value)
        .ok()
        .and_then(|value| value.as_str().map(ToOwned::to_owned))
}

fn notion_title(value: &str) -> Value {
    json!({
        "title": [{"text": {"content": truncate(value, 2000)}}]
    })
}

fn notion_rich_text(value: Option<&str>) -> Value {
    if let Some(value) = value
        && !value.trim().is_empty()
    {
        return json!({
            "rich_text": [{"text": {"content": truncate(value, 2000)}}]
        });
    }
    json!({ "rich_text": [] })
}

fn notion_number(value: Option<f64>) -> Value {
    json!({ "number": value })
}

fn notion_select(value: Option<&str>) -> Value {
    if let Some(value) = value
        && !value.trim().is_empty()
    {
        return json!({ "select": { "name": value } });
    }
    json!({ "select": null })
}

fn notion_url(value: Option<&str>) -> Value {
    json!({ "url": value })
}

fn notion_files(urls: &[String]) -> Value {
    let files = urls
        .iter()
        .enumerate()
        .map(|(idx, url)| {
            json!({
                "type": "external",
                "name": format!("Image {}", idx + 1),
                "external": { "url": url },
            })
        })
        .collect::<Vec<_>>();
    json!({ "files": files })
}

fn truncate(value: &str, max_len: usize) -> String {
    if value.chars().count() <= max_len {
        value.to_owned()
    } else {
        value.chars().take(max_len).collect()
    }
}
