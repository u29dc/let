#![forbid(unsafe_code)]

use std::path::Path;
use std::sync::Arc;
use std::time::Duration;

use image::imageops::FilterType;
use image::{DynamicImage, GenericImageView, ImageBuffer, Rgb, codecs::jpeg::JpegEncoder};
use tokio::sync::Semaphore;

use crate::schema::listing::{Listing, RemoteLocalAsset};

use super::cache::{
    AssetKind, asset_filename, ensure_listing_cache_dir, local_asset_path, short_hash,
};
use super::maps::{MapFetchOptions, fetch_map_views};

#[derive(Debug, Clone)]
pub struct MediaNormalizationConfig {
    pub photo_landscape_width: u32,
    pub photo_landscape_height: u32,
    pub photo_portrait_width: u32,
    pub photo_portrait_height: u32,
    pub aux_width: u32,
    pub aux_height: u32,
    pub map_width: u32,
    pub map_height: u32,
    pub photo_quality: u8,
    pub aux_quality: u8,
    pub map_quality: u8,
    pub timeout: Duration,
    pub max_retries: usize,
    pub download_concurrency: usize,
    pub process_concurrency: usize,
    pub download_maps: bool,
    pub download_floorplan: bool,
    pub download_epc_asset: bool,
    pub mapbox_access_token: Option<String>,
}

impl MediaNormalizationConfig {
    pub fn profile_hash(&self) -> String {
        short_hash(&format!(
            "pl{}x{}-pp{}x{}-aux{}x{}-map{}x{}-pq{}-aq{}-mq{}",
            self.photo_landscape_width,
            self.photo_landscape_height,
            self.photo_portrait_width,
            self.photo_portrait_height,
            self.aux_width,
            self.aux_height,
            self.map_width,
            self.map_height,
            self.photo_quality,
            self.aux_quality,
            self.map_quality,
        ))
    }
}

#[derive(Debug, Clone, Default)]
pub struct MediaStageStats {
    pub photos_downloaded: usize,
    pub photos_skipped: usize,
    pub photos_failed: usize,
    pub floorplan_downloaded: usize,
    pub floorplan_skipped: usize,
    pub floorplan_failed: usize,
    pub epc_downloaded: usize,
    pub epc_skipped: usize,
    pub epc_failed: usize,
    pub maps_downloaded: usize,
    pub maps_skipped: usize,
    pub maps_failed: usize,
}

pub async fn populate_listing_media(
    client: &reqwest::Client,
    listing: &mut Listing,
    cache_root: &Path,
    config: &MediaNormalizationConfig,
) -> MediaStageStats {
    let mut stats = MediaStageStats::default();

    let listing_dir = match ensure_listing_cache_dir(cache_root, listing) {
        Ok(path) => path,
        Err(_) => {
            stats.photos_failed += listing.images.len();
            return stats;
        }
    };

    let profile_hash = config.profile_hash();
    let media_client = reqwest::Client::builder()
        .redirect(reqwest::redirect::Policy::none())
        .build()
        .unwrap_or_else(|_| client.clone());

    process_listing_images(
        &media_client,
        listing,
        &listing_dir,
        &profile_hash,
        config,
        &mut stats,
    )
    .await;

    let listing_snapshot = listing.clone();
    if config.download_floorplan {
        process_single_asset(
            &media_client,
            &listing_snapshot,
            &listing_dir,
            &profile_hash,
            AssetKind::Floorplan,
            &mut listing.floorplan,
            config,
            &mut stats.floorplan_downloaded,
            &mut stats.floorplan_skipped,
            &mut stats.floorplan_failed,
        )
        .await;
    }

    if config.download_epc_asset {
        process_single_asset(
            &media_client,
            &listing_snapshot,
            &listing_dir,
            &profile_hash,
            AssetKind::Epc,
            &mut listing.epc,
            config,
            &mut stats.epc_downloaded,
            &mut stats.epc_skipped,
            &mut stats.epc_failed,
        )
        .await;
    }

    let map_outcome = fetch_map_views(
        client,
        listing,
        cache_root,
        &profile_hash,
        &MapFetchOptions {
            enabled: config.download_maps,
            access_token: config.mapbox_access_token.clone(),
            width: config.map_width,
            height: config.map_height,
            jpeg_quality: config.map_quality,
            max_retries: config.max_retries,
            timeout: config.timeout,
        },
    )
    .await;

    if let Some(satellite) = map_outcome.satellite {
        listing.map_views.satellite = satellite;
    }
    if let Some(street) = map_outcome.street {
        listing.map_views.street = street;
    }
    stats.maps_downloaded += map_outcome.stats.downloaded;
    stats.maps_skipped += map_outcome.stats.skipped;
    stats.maps_failed += map_outcome.stats.failed;

    stats
}

async fn process_listing_images(
    client: &reqwest::Client,
    listing: &mut Listing,
    listing_dir: &Path,
    profile_hash: &str,
    config: &MediaNormalizationConfig,
    stats: &mut MediaStageStats,
) {
    let download_limit = Arc::new(Semaphore::new(config.download_concurrency.max(1)));
    let process_limit = Arc::new(Semaphore::new(config.process_concurrency.max(1)));

    let mut join_set = tokio::task::JoinSet::new();
    let remotes = listing
        .images
        .iter()
        .enumerate()
        .map(|(index, image)| (index, image.remote.clone()))
        .collect::<Vec<_>>();

    for (index, remote) in remotes {
        let filename = asset_filename(
            listing,
            AssetKind::Photo,
            &remote,
            profile_hash,
            Some(index),
        );
        let output_path = listing_dir.join(&filename);
        let local = local_asset_path(listing, &filename);

        if cached_image_is_valid(&output_path) {
            stats.photos_skipped += 1;
            if let Some(slot) = listing.images.get_mut(index) {
                slot.local = Some(local);
            }
            continue;
        }

        let client = client.clone();
        let download_limit = Arc::clone(&download_limit);
        let process_limit = Arc::clone(&process_limit);
        let config = config.clone();

        join_set.spawn(async move {
            let _download_permit = download_limit
                .acquire_owned()
                .await
                .map_err(|_| AssetProcessError::Cancelled)?;

            let bytes = download_with_retries(&client, &remote, &config)
                .await
                .ok_or(AssetProcessError::DownloadFailed)?;

            let _process_permit = process_limit
                .acquire_owned()
                .await
                .map_err(|_| AssetProcessError::Cancelled)?;
            let normalized = tokio::task::spawn_blocking(move || normalize_photo(&bytes, &config))
                .await
                .map_err(|_| AssetProcessError::ProcessFailed)??;

            write_atomically(&output_path, &normalized)
                .map_err(|_| AssetProcessError::WriteFailed)?;

            Ok::<AssetProcessSuccess, AssetProcessError>(AssetProcessSuccess { index, local })
        });
    }

    while let Some(result) = join_set.join_next().await {
        match result {
            Ok(Ok(success)) => {
                stats.photos_downloaded += 1;
                if let Some(slot) = listing.images.get_mut(success.index) {
                    slot.local = Some(success.local);
                }
            }
            Ok(Err(_)) | Err(_) => {
                stats.photos_failed += 1;
            }
        }
    }
}

#[allow(clippy::too_many_arguments)]
async fn process_single_asset(
    client: &reqwest::Client,
    listing: &Listing,
    listing_dir: &Path,
    profile_hash: &str,
    kind: AssetKind,
    asset: &mut RemoteLocalAsset,
    config: &MediaNormalizationConfig,
    downloaded: &mut usize,
    skipped: &mut usize,
    failed: &mut usize,
) {
    let Some(remote) = asset.remote.clone() else {
        return;
    };

    let filename = asset_filename(listing, kind, &remote, profile_hash, None);
    let output_path = listing_dir.join(&filename);
    let local = local_asset_path(listing, &filename);

    if cached_image_is_valid(&output_path) {
        asset.local = Some(local);
        *skipped += 1;
        return;
    }

    let Some(bytes) = download_with_retries(client, &remote, config).await else {
        *failed += 1;
        return;
    };

    let config_cloned = config.clone();
    let normalized = match tokio::task::spawn_blocking(move || {
        normalize_aux_asset(&bytes, &config_cloned)
    })
    .await
    {
        Ok(Ok(bytes)) => bytes,
        _ => {
            *failed += 1;
            return;
        }
    };

    if write_atomically(&output_path, &normalized).is_err() {
        *failed += 1;
        return;
    }

    asset.local = Some(local);
    *downloaded += 1;
}

async fn download_with_retries(
    client: &reqwest::Client,
    url: &str,
    config: &MediaNormalizationConfig,
) -> Option<Vec<u8>> {
    if !is_allowed_remote_media_url(url) {
        return None;
    }

    let max_retries = config.max_retries.max(1);
    for attempt in 1..=max_retries {
        let response = client
            .get(url)
            .timeout(config.timeout)
            .header(
                reqwest::header::USER_AGENT,
                "Mozilla/5.0 (Macintosh; Intel Mac OS X 10_15_7)",
            )
            .send()
            .await;

        match response {
            Ok(response) if response.status().is_success() => match response.bytes().await {
                Ok(bytes) => return Some(bytes.to_vec()),
                Err(_) if attempt < max_retries => {
                    tokio::time::sleep(Duration::from_millis((attempt as u64) * 500)).await;
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
                tokio::time::sleep(Duration::from_millis((attempt as u64) * 900)).await;
            }
            Err(_) => return None,
        }
    }

    None
}

fn is_allowed_remote_media_url(raw: &str) -> bool {
    let Ok(url) = url::Url::parse(raw) else {
        return false;
    };
    if url.scheme() != "https" {
        return false;
    }
    let Some(host) = url.host_str().map(str::to_ascii_lowercase) else {
        return false;
    };
    host == "media.rightmove.co.uk" || host.ends_with(".media.rightmove.co.uk")
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

fn normalize_photo(
    bytes: &[u8],
    config: &MediaNormalizationConfig,
) -> Result<Vec<u8>, AssetProcessError> {
    let image = image::load_from_memory(bytes).map_err(|_| AssetProcessError::DecodeFailed)?;
    let (width, height) = image.dimensions();
    let (target_w, target_h) = if width >= height {
        (config.photo_landscape_width, config.photo_landscape_height)
    } else {
        (config.photo_portrait_width, config.photo_portrait_height)
    };

    let normalized = normalize_cover(image, target_w, target_h);
    encode_jpeg(normalized, config.photo_quality)
}

fn normalize_aux_asset(
    bytes: &[u8],
    config: &MediaNormalizationConfig,
) -> Result<Vec<u8>, AssetProcessError> {
    let image = image::load_from_memory(bytes).map_err(|_| AssetProcessError::DecodeFailed)?;
    let normalized = normalize_contain(image, config.aux_width, config.aux_height);
    encode_jpeg(normalized, config.aux_quality)
}

fn encode_jpeg(image: DynamicImage, quality: u8) -> Result<Vec<u8>, AssetProcessError> {
    let mut output = Vec::new();
    JpegEncoder::new_with_quality(&mut output, quality.clamp(40, 95))
        .encode_image(&image)
        .map_err(|_| AssetProcessError::EncodeFailed)?;
    Ok(output)
}

fn normalize_cover(image: DynamicImage, target_w: u32, target_h: u32) -> DynamicImage {
    let (width, height) = image.dimensions();

    if width == target_w && height == target_h {
        return image;
    }

    let scale_w = target_w as f64 / width as f64;
    let scale_h = target_h as f64 / height as f64;
    let scale = scale_w.max(scale_h);

    let resized_w = ((width as f64) * scale).round().max(1.0) as u32;
    let resized_h = ((height as f64) * scale).round().max(1.0) as u32;

    let resized = image.resize_exact(resized_w, resized_h, FilterType::CatmullRom);
    let x = resized_w.saturating_sub(target_w) / 2;
    let y = resized_h.saturating_sub(target_h) / 2;
    resized.crop_imm(x, y, target_w, target_h)
}

fn normalize_contain(image: DynamicImage, target_w: u32, target_h: u32) -> DynamicImage {
    let (width, height) = image.dimensions();

    if width == 0 || height == 0 {
        return DynamicImage::ImageRgb8(ImageBuffer::from_pixel(
            target_w.max(1),
            target_h.max(1),
            Rgb([255, 255, 255]),
        ));
    }

    let scale_w = target_w as f64 / width as f64;
    let scale_h = target_h as f64 / height as f64;
    let scale = scale_w.min(scale_h);

    let resized_w = ((width as f64) * scale).round().max(1.0) as u32;
    let resized_h = ((height as f64) * scale).round().max(1.0) as u32;
    let resized = image.resize_exact(resized_w, resized_h, FilterType::CatmullRom);

    let mut canvas = ImageBuffer::from_pixel(target_w, target_h, Rgb([255, 255, 255]));
    let offset_x = (target_w.saturating_sub(resized_w)) / 2;
    let offset_y = (target_h.saturating_sub(resized_h)) / 2;
    image::imageops::replace(
        &mut canvas,
        &resized.to_rgb8(),
        i64::from(offset_x),
        i64::from(offset_y),
    );

    DynamicImage::ImageRgb8(canvas)
}

#[derive(Debug)]
struct AssetProcessSuccess {
    index: usize,
    local: String,
}

#[derive(Debug)]
enum AssetProcessError {
    DownloadFailed,
    DecodeFailed,
    ProcessFailed,
    EncodeFailed,
    WriteFailed,
    Cancelled,
}

#[cfg(test)]
mod tests {
    use image::{ImageBuffer, Rgb};

    use super::{
        MediaNormalizationConfig, cached_image_is_valid, is_allowed_remote_media_url,
        normalize_contain, normalize_cover,
    };

    #[test]
    fn cover_resizes_to_exact_dimensions() {
        let input =
            image::DynamicImage::ImageRgb8(ImageBuffer::from_pixel(400, 300, Rgb([0, 0, 0])));
        let output = normalize_cover(input, 1200, 900);
        assert_eq!(output.width(), 1200);
        assert_eq!(output.height(), 900);
    }

    #[test]
    fn contain_resizes_to_exact_canvas() {
        let input =
            image::DynamicImage::ImageRgb8(ImageBuffer::from_pixel(300, 400, Rgb([0, 0, 0])));
        let output = normalize_contain(input, 1200, 900);
        assert_eq!(output.width(), 1200);
        assert_eq!(output.height(), 900);
    }

    #[test]
    fn profile_hash_changes_with_quality() {
        let mut config = sample_config();
        let first = config.profile_hash();
        config.photo_quality = 90;
        let second = config.profile_hash();
        assert_ne!(first, second);
    }

    #[test]
    fn media_url_allowlist_rejects_local_and_non_https_urls() {
        assert!(is_allowed_remote_media_url(
            "https://media.rightmove.co.uk/dir/photo.jpg"
        ));
        assert!(!is_allowed_remote_media_url("http://127.0.0.1/probe.jpg"));
        assert!(!is_allowed_remote_media_url(
            "https://example.com/photo.jpg"
        ));
    }

    #[test]
    fn cached_image_validation_rejects_empty_files() {
        let temp = tempfile::TempDir::new().expect("temp dir");
        let empty = temp.path().join("empty.jpg");
        std::fs::write(&empty, []).expect("write empty file");
        assert!(!cached_image_is_valid(&empty));

        let valid = temp.path().join("valid.jpg");
        image::DynamicImage::ImageRgb8(ImageBuffer::from_pixel(4, 4, Rgb([0, 0, 0])))
            .save(&valid)
            .expect("write valid image");
        assert!(cached_image_is_valid(&valid));
    }

    fn sample_config() -> MediaNormalizationConfig {
        MediaNormalizationConfig {
            photo_landscape_width: 1200,
            photo_landscape_height: 900,
            photo_portrait_width: 900,
            photo_portrait_height: 1200,
            aux_width: 1200,
            aux_height: 900,
            map_width: 1200,
            map_height: 1200,
            photo_quality: 82,
            aux_quality: 85,
            map_quality: 85,
            timeout: std::time::Duration::from_secs(20),
            max_retries: 2,
            download_concurrency: 4,
            process_concurrency: 2,
            download_maps: true,
            download_floorplan: true,
            download_epc_asset: true,
            mapbox_access_token: None,
        }
    }
}
