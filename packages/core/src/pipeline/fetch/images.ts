/**
 * Image download and processing for property listings
 *
 * Downloads property images from Rightmove CDN, converts to JPEG,
 * and caps maximum dimensions for consistent AI analysis.
 * Uses @napi-rs/image for Rust-based processing.
 *
 * Downloads are SEQUENTIAL (not concurrent) for simplicity and reliability.
 */

import { existsSync, mkdirSync } from 'node:fs';
import { join } from 'node:path';
import { log } from '@let/core/utils/logger';
import { Transformer } from '@napi-rs/image';
import { sleep } from './html.js';

// =============================================================================
// CONFIGURATION
// =============================================================================

/** Timeout for image fetch requests (ms) */
const IMAGE_FETCH_TIMEOUT_MS = 5_000;

/** Timeout for image processing (ms) - first call has cold start overhead */
const IMAGE_PROCESS_TIMEOUT_MS = 5_000;

const IMAGE_CONFIG = {
	/** Maximum dimension (width for landscape, height for portrait) */
	maxDimension: 1200,
	/** JPEG quality (0-100) */
	quality: 80,
	/** Delay between downloads (ms) */
	delayMs: 200,
	/** Maximum retries per image */
	maxRetries: 2,
	/** Retry delay base (ms) */
	retryDelayMs: 1000,
} as const;

// =============================================================================
// TYPES
// =============================================================================

/** Image entry with remote URL and local filename */
export type ImageEntry = {
	remote: string;
	local: string | null;
};

/** Result of downloading listing images */
export type ImageDownloadResult = {
	success: boolean;
	images: ImageEntry[];
	floorplan: { remote: string | null; local: string | null };
	epc: { remote: string | null; local: string | null };
	stats: {
		downloaded: number;
		skipped: number;
		failed: number;
	};
};

// =============================================================================
// UTILITIES
// =============================================================================

/**
 * Generate deterministic hash from URL (first 8 chars of SHA-256)
 */
function hashUrl(url: string): string {
	const hash = new Bun.CryptoHasher('sha256').update(url).digest('hex');
	return hash.slice(0, 8);
}

/**
 * Generate deterministic image filename from URL
 * Format: {id}-{type}-{urlhash}.jpg
 */
export function generateImageFilename(id: string, type: 'photo' | 'floorplan' | 'epc', remoteUrl: string): string {
	return `${id}-${type}-${hashUrl(remoteUrl)}.jpg`;
}

/**
 * Get or create the listing cache directory
 */
export function getListingCacheDir(id: string, cacheDir: string): string {
	const listingDir = join(cacheDir, id);
	if (!existsSync(listingDir)) {
		mkdirSync(listingDir, { recursive: true });
	}
	return listingDir;
}

/**
 * Check if an image is already cached
 * Returns the filename if cached, null otherwise
 */
function getCachedFilename(listingDir: string, id: string, type: 'photo' | 'floorplan' | 'epc', remoteUrl: string): string | null {
	const expectedFilename = generateImageFilename(id, type, remoteUrl);
	const fullPath = join(listingDir, expectedFilename);
	return existsSync(fullPath) ? expectedFilename : null;
}

// =============================================================================
// IMAGE PROCESSING
// =============================================================================

/** User agent for image downloads */
const USER_AGENT = 'Mozilla/5.0 (Macintosh; Intel Mac OS X 10_15_7) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/120.0.0.0 Safari/537.36';

/** Result of a single fetch attempt */
type FetchAttemptResult = { ok: true; buffer: Buffer } | { ok: false };

/**
 * Create a timeout promise that can be cancelled
 */
function createTimeout<T>(ms: number, value: T, onTimeout?: () => void): { promise: Promise<T>; cancel: () => void } {
	let timeoutId: Timer;
	const promise = new Promise<T>((resolve) => {
		timeoutId = setTimeout(() => {
			onTimeout?.();
			resolve(value);
		}, ms);
	});
	return { promise, cancel: () => clearTimeout(timeoutId) };
}

/**
 * Read response body with timeout protection
 * The fetch() timeout only covers connection, NOT body reading - this fixes hangs where
 * server sends headers but trickles body slowly or holds connection open
 */
async function readBodyWithTimeout(response: Response, timeoutMs: number, label: string): Promise<Buffer | null> {
	const bodyPromise = response.arrayBuffer().then((ab) => Buffer.from(ab));
	const timeout = createTimeout(timeoutMs, null, () => {
		log.fetchImages.warn(`[${label}] Body read timeout after ${timeoutMs}ms`);
	});
	const result = await Promise.race([bodyPromise, timeout.promise]);
	timeout.cancel();
	return result;
}

/**
 * Wrap image processing with timeout to catch hung native code
 */
async function processWithTimeout(buffer: Buffer, label: string): Promise<Buffer | null> {
	const processPromise = processImage(buffer, label);
	const timeout = createTimeout(IMAGE_PROCESS_TIMEOUT_MS, null, () => {
		log.fetchImages.warn(`[${label}] Processing timeout after ${IMAGE_PROCESS_TIMEOUT_MS}ms`);
	});
	const result = await Promise.race([processPromise, timeout.promise]);
	timeout.cancel();
	return result;
}

/**
 * Execute a single fetch attempt with timeout on BOTH connection and body reading
 */
async function attemptFetch(url: string, label: string, attempt: number): Promise<FetchAttemptResult> {
	const controller = new AbortController();
	const connectionTimeout = setTimeout(() => controller.abort(), IMAGE_FETCH_TIMEOUT_MS);

	try {
		const response = await fetch(url, {
			signal: controller.signal,
			headers: { 'User-Agent': USER_AGENT, Accept: 'image/webp,image/jpeg,image/png,*/*', Referer: 'https://www.rightmove.co.uk/' },
		});

		clearTimeout(connectionTimeout); // Connection succeeded, clear connection timeout

		if (!response.ok) {
			log.fetchImages.warn(`[${label}] HTTP ${response.status}`, { attempt });
			return { ok: false };
		}

		// Body reading has its own timeout (the root cause of hangs)
		const buffer = await readBodyWithTimeout(response, IMAGE_FETCH_TIMEOUT_MS, label);
		if (!buffer) {
			return { ok: false };
		}

		log.fetchImages.debug(`[${label}] Got ${buffer.length} bytes`);
		return { ok: true, buffer };
	} catch (e) {
		clearTimeout(connectionTimeout);
		const isTimeout = e instanceof Error && e.name === 'AbortError';
		log.fetchImages.warn(`[${label}] ${isTimeout ? 'Connection timeout' : 'Error'}`, {
			attempt,
			error: !isTimeout && e instanceof Error ? e.message : undefined,
		});
		return { ok: false };
	}
}

/**
 * Download a single image with retries
 */
async function downloadImage(url: string, label: string): Promise<Buffer | null> {
	for (let attempt = 1; attempt <= IMAGE_CONFIG.maxRetries; attempt++) {
		log.fetchImages.debug(`[${label}] Fetch attempt ${attempt}`, { url: url.slice(0, 60) });

		const result = await attemptFetch(url, label, attempt);
		if (result.ok) return result.buffer;

		if (attempt < IMAGE_CONFIG.maxRetries) {
			await sleep(IMAGE_CONFIG.retryDelayMs * attempt);
		}
	}
	return null;
}

/**
 * Process image buffer: downscale if oversized and convert to JPEG
 * No upscaling — Rightmove images are already web-optimized
 */
async function processImage(buffer: Buffer, label: string): Promise<Buffer> {
	log.fetchImages.debug(`[${label}] Processing image`);

	const transformer = new Transformer(buffer);
	const metadata = await transformer.metadata();
	const width = metadata.width ?? 0;
	const height = metadata.height ?? 0;

	log.fetchImages.debug(`[${label}] Image size: ${width}x${height}`);

	const isLandscape = width >= height;

	if (isLandscape) {
		if (width > IMAGE_CONFIG.maxDimension) {
			return transformer.resize(IMAGE_CONFIG.maxDimension).jpeg(IMAGE_CONFIG.quality);
		}
	} else {
		if (height > IMAGE_CONFIG.maxDimension) {
			return transformer.resize({ width: 99999, height: IMAGE_CONFIG.maxDimension }).jpeg(IMAGE_CONFIG.quality);
		}
	}

	return transformer.jpeg(IMAGE_CONFIG.quality);
}

/**
 * Download and process a single image (sequential, no concurrency)
 */
async function downloadAndProcessImage(url: string, outputPath: string, label: string): Promise<boolean> {
	try {
		// Small delay between downloads
		await sleep(IMAGE_CONFIG.delayMs);

		// Download
		log.fetchImages.info(`[${label}] Downloading...`);
		const buffer = await downloadImage(url, label);
		if (!buffer) {
			log.fetchImages.warn(`[${label}] Download failed`);
			return false;
		}
		log.fetchImages.info(`[${label}] Downloaded ${(buffer.length / 1024).toFixed(0)}KB`);

		// Process (with timeout to catch hung native code)
		log.fetchImages.info(`[${label}] Processing...`);
		const processed = await processWithTimeout(buffer, label);
		if (!processed) {
			log.fetchImages.warn(`[${label}] Processing failed or timed out`);
			return false;
		}
		log.fetchImages.info(`[${label}] Processed to ${(processed.length / 1024).toFixed(0)}KB`);

		// Write
		await Bun.write(outputPath, processed);
		log.fetchImages.info(`[${label}] Saved`);

		return true;
	} catch (e) {
		log.fetchImages.error(`[${label}] Failed`, { error: e instanceof Error ? e.message : String(e) });
		return false;
	}
}

// =============================================================================
// PUBLIC API
// =============================================================================

/** Stats tracker for image downloads */
type DownloadStats = { downloaded: number; skipped: number; failed: number };

/**
 * Process a single image: check cache or download
 * Returns the local filename on success, null on failure
 */
async function processSingleImage(id: string, remoteUrl: string, type: 'photo' | 'floorplan' | 'epc', label: string, listingDir: string, stats: DownloadStats): Promise<string | null> {
	const cached = getCachedFilename(listingDir, id, type, remoteUrl);
	if (cached) {
		stats.skipped++;
		return cached;
	}

	const filename = generateImageFilename(id, type, remoteUrl);
	const outputPath = join(listingDir, filename);

	const success = await downloadAndProcessImage(remoteUrl, outputPath, label);

	if (success) {
		stats.downloaded++;
		return filename;
	}

	stats.failed++;
	log.fetchImages.debug(`Failed ${label}`, { id });
	return null;
}

/**
 * Download and process all images for a listing (SEQUENTIAL)
 */
export async function downloadListingImages(
	id: string,
	images: ImageEntry[],
	floorplan: { remote: string | null; local: string | null },
	epc: { remote: string | null; local: string | null },
	cacheDir: string,
): Promise<ImageDownloadResult> {
	const listingDir = getListingCacheDir(id, cacheDir);
	const stats: DownloadStats = { downloaded: 0, skipped: 0, failed: 0 };
	const updatedImages: ImageEntry[] = [];
	const updatedFloorplan = { ...floorplan };
	const updatedEpc = { ...epc };

	const total = images.length;
	log.fetchImages.info('Starting image downloads', { id, photos: total, hasFloorplan: !!floorplan.remote, hasEpc: !!epc.remote });

	// Process photos SEQUENTIALLY
	let photoNum = 0;
	for (const img of images) {
		photoNum++;
		const label = `photo ${photoNum}/${total}`;
		const local = await processSingleImage(id, img.remote, 'photo', label, listingDir, stats);
		updatedImages.push({ remote: img.remote, local });
	}

	/**
	 * Floorplan and EPC images are intentionally skipped.
	 *
	 * These document-type images (PDF-sourced, complex layouts) cause:
	 * - Processing hangs in the image transformer
	 * - Memory issues with large/complex documents
	 * - Minimal value-add (EPC data fetched from API, floorplans rarely needed)
	 *
	 * The remote URLs remain available for direct viewing.
	 * The local fields will always be null.
	 */
	// if (floorplan.remote) {
	// 	updatedFloorplan.local = await processSingleImage(id, floorplan.remote, 'floorplan', 'floorplan', listingDir, stats);
	// }
	// if (epc.remote) {
	// 	updatedEpc.local = await processSingleImage(id, epc.remote, 'epc', 'epc', listingDir, stats);
	// }

	log.fetchImages.info('Image download complete', { id, ...stats });

	return {
		success: stats.failed === 0,
		images: updatedImages,
		floorplan: updatedFloorplan,
		epc: updatedEpc,
		stats,
	};
}
