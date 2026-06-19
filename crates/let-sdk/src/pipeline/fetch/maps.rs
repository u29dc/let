#![forbid(unsafe_code)]

use std::path::Path;
use std::time::Duration;

use image::imageops::FilterType;
use image::{DynamicImage, GenericImageView, codecs::jpeg::JpegEncoder};

use crate::schema::listing::{Listing, RemoteLocalAsset};

use super::cache::{AssetKind, asset_filename, ensure_listing_cache_dir, local_asset_path};

const MAP_STYLE_SATELLITE: &str = "mapbox/satellite-v9";
const MAP_STYLE_STREET: &str = "mapbox/streets-v12";

#[derive(Debug, Clone)]
pub struct MapFetchOptions {
    pub enabled: bool,
    pub access_token: Option<String>,
    pub width: u32,
    pub height: u32,
    pub jpeg_quality: u8,
    pub max_retries: usize,
    pub timeout: Duration,
}

#[derive(Debug, Clone, Default)]
pub struct MapFetchStats {
    pub downloaded: usize,
    pub skipped: usize,
    pub failed: usize,
}

#[derive(Debug, Clone, Default)]
pub struct MapFetchOutcome {
    pub satellite: Option<RemoteLocalAsset>,
    pub street: Option<RemoteLocalAsset>,
    pub stats: MapFetchStats,
}

pub async fn fetch_map_views(
    client: &reqwest::Client,
    listing: &Listing,
    cache_root: &Path,
    profile_hash: &str,
    options: &MapFetchOptions,
) -> MapFetchOutcome {
    let mut outcome = MapFetchOutcome::default();

    if !options.enabled {
        return outcome;
    }
    let Some(token) = options.access_token.as_deref() else {
        return outcome;
    };

    let listing_dir = match ensure_listing_cache_dir(cache_root, listing) {
        Ok(path) => path,
        Err(_) => return outcome,
    };

    let satellite = fetch_one_map(
        client,
        listing,
        &listing_dir,
        profile_hash,
        token,
        AssetKind::MapSatellite,
        MAP_STYLE_SATELLITE,
        options,
        &mut outcome.stats,
    )
    .await;
    let street = fetch_one_map(
        client,
        listing,
        &listing_dir,
        profile_hash,
        token,
        AssetKind::MapStreet,
        MAP_STYLE_STREET,
        options,
        &mut outcome.stats,
    )
    .await;

    outcome.satellite = satellite;
    outcome.street = street;
    outcome
}

#[allow(clippy::too_many_arguments)]
async fn fetch_one_map(
    client: &reqwest::Client,
    listing: &Listing,
    listing_dir: &Path,
    profile_hash: &str,
    access_token: &str,
    kind: AssetKind,
    style: &str,
    options: &MapFetchOptions,
    stats: &mut MapFetchStats,
) -> Option<RemoteLocalAsset> {
    let public_url = build_public_map_url(style, listing.location.lat, listing.location.lng);
    let filename = asset_filename(listing, kind, &public_url, profile_hash, None);
    let output_path = listing_dir.join(&filename);
    let local = local_asset_path(listing, &filename);

    if cached_image_is_valid(&output_path) {
        stats.skipped += 1;
        return Some(RemoteLocalAsset {
            remote: Some(public_url),
            local: Some(local),
        });
    }

    let secure_url = format!("{public_url}?access_token={access_token}");
    let bytes = match download_with_retries(client, &secure_url, options).await {
        Some(bytes) => bytes,
        None => {
            stats.failed += 1;
            return Some(RemoteLocalAsset {
                remote: Some(public_url),
                local: None,
            });
        }
    };

    let image = match image::load_from_memory(&bytes) {
        Ok(image) => image,
        Err(_) => {
            stats.failed += 1;
            return Some(RemoteLocalAsset {
                remote: Some(public_url),
                local: None,
            });
        }
    };

    let normalized = normalize_exact(image, options.width, options.height);
    let mut encoded = Vec::new();
    if JpegEncoder::new_with_quality(&mut encoded, options.jpeg_quality)
        .encode_image(&normalized)
        .is_err()
    {
        stats.failed += 1;
        return Some(RemoteLocalAsset {
            remote: Some(public_url),
            local: None,
        });
    }

    if write_atomically(&output_path, &encoded).is_err() {
        stats.failed += 1;
        return Some(RemoteLocalAsset {
            remote: Some(public_url),
            local: None,
        });
    }

    stats.downloaded += 1;
    Some(RemoteLocalAsset {
        remote: Some(public_url),
        local: Some(local),
    })
}

fn cached_image_is_valid(path: &Path) -> bool {
    path.is_file() && image::open(path).is_ok()
}

fn write_atomically(path: &Path, bytes: &[u8]) -> std::io::Result<()> {
    let file_name = path
        .file_name()
        .map(|name| name.to_string_lossy())
        .unwrap_or_else(|| "asset".into());
    let temp_path = path.with_file_name(format!("{file_name}.{}.tmp", uuid::Uuid::new_v4()));
    let result = (|| {
        std::fs::write(&temp_path, bytes)?;
        if path.exists() {
            std::fs::remove_file(path)?;
        }
        std::fs::rename(&temp_path, path)
    })();
    if result.is_err() {
        let _ = std::fs::remove_file(&temp_path);
    }
    result
}

async fn download_with_retries(
    client: &reqwest::Client,
    url: &str,
    options: &MapFetchOptions,
) -> Option<Vec<u8>> {
    let max_retries = options.max_retries.max(1);
    for attempt in 1..=max_retries {
        let response = client.get(url).timeout(options.timeout).send().await;
        match response {
            Ok(response) if response.status().is_success() => match response.bytes().await {
                Ok(bytes) => return Some(bytes.to_vec()),
                Err(_) if attempt < max_retries => {
                    tokio::time::sleep(Duration::from_millis((attempt as u64) * 700)).await;
                }
                Err(_) => return None,
            },
            Ok(response)
                if (response.status().as_u16() == 429 || response.status().is_server_error())
                    && attempt < max_retries =>
            {
                tokio::time::sleep(Duration::from_millis((attempt as u64) * 1000)).await;
            }
            Ok(_) => return None,
            Err(_) if attempt < max_retries => {
                tokio::time::sleep(Duration::from_millis((attempt as u64) * 1000)).await;
            }
            Err(_) => return None,
        }
    }
    None
}

fn normalize_exact(image: DynamicImage, target_width: u32, target_height: u32) -> DynamicImage {
    let oriented = image;
    let (width, height) = oriented.dimensions();

    if width == target_width && height == target_height {
        return oriented;
    }

    // Cover resize then center-crop to a stable exact size.
    let scale_w = target_width as f64 / width as f64;
    let scale_h = target_height as f64 / height as f64;
    let scale = scale_w.max(scale_h);

    let resized_w = ((width as f64) * scale).round().max(1.0) as u32;
    let resized_h = ((height as f64) * scale).round().max(1.0) as u32;
    let resized = oriented.resize_exact(resized_w, resized_h, FilterType::CatmullRom);

    let x = resized_w.saturating_sub(target_width) / 2;
    let y = resized_h.saturating_sub(target_height) / 2;
    resized.crop_imm(x, y, target_width, target_height)
}

fn build_public_map_url(style: &str, lat: f64, lng: f64) -> String {
    let marker = format!("pin-l+f00({lng},{lat})");
    format!("https://api.mapbox.com/styles/v1/{style}/static/{marker}/{lng},{lat},15/600x600@2x")
}

#[cfg(test)]
mod tests {
    use super::build_public_map_url;

    #[test]
    fn map_url_contains_coordinates() {
        let url = build_public_map_url("mapbox/streets-v12", 53.1, -2.2);
        assert!(url.contains("53.1"));
        assert!(url.contains("-2.2"));
    }
}
