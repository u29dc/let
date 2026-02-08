/**
 * Map view fetching from Mapbox Static Images API
 *
 * Fetches two complementary views for neighborhood assessment:
 * - satellite: Aerial imagery (satellite-v9) - neighborhood vibes, green spaces, density
 * - street: Labeled map (streets-v12) - street names, park labels, POIs, transport
 *
 * Zoom: 15 (~3m/pixel at UK latitudes, ~10 min walking radius)
 * Size: 600x600@2x (1200px actual)
 * Marker: Large red pin at property coordinates
 */

import { existsSync, mkdirSync } from 'node:fs';
import { join } from 'node:path';
import { calculateBackoff, sleep } from '@let/core/utils/http';
import { log } from '@let/core/utils/logger';

// =============================================================================
// CONFIGURATION
// =============================================================================

const MAP_STYLES = {
	satellite: 'mapbox/satellite-v9',
	street: 'mapbox/streets-v12',
} as const;

/** Timeout for map fetch requests (ms) */
const MAP_FETCH_TIMEOUT_MS = 10_000;

const MAP_CONFIG = {
	/** Zoom level (15 = ~3m/pixel, ~10 min walking radius) */
	zoom: 15,
	/** Image width in pixels */
	width: 600,
	/** Image height in pixels */
	height: 600,
	/** Request 2x resolution */
	retina: true,
} as const;

/** Maximum retries for Mapbox requests */
const MAP_MAX_RETRIES = 2;

/** Base delay for retry backoff (ms) */
const MAP_RETRY_DELAY_MS = 1500;

export type MapViewType = 'satellite' | 'street';

// =============================================================================
// TYPES
// =============================================================================

/** Single map view entry with remote URL and local cached filename */
export type MapViewEntry = {
	/** Mapbox Static Images API URL (without access token) */
	remote: string | null;
	/** Local cached filename, null if not cached */
	local: string | null;
};

/** Both map views for a listing */
export type MapViews = {
	/** Satellite/aerial imagery (no labels) */
	satellite: MapViewEntry;
	/** Street map with labels (POIs, streets, parks) */
	street: MapViewEntry;
};

/** Result of fetching map views */
export type MapViewsFetchResult = { success: true; mapViews: MapViews } | { success: false; error: string };

// =============================================================================
// STATE
// =============================================================================

/** Track if warning has been shown for missing token */
let warnedMissingToken = false;

// =============================================================================
// UTILITIES
// =============================================================================

/**
 * Get Mapbox access token from environment
 * Warns once if missing
 */
function getAccessToken(): string | null {
	const token = process.env['MAPBOX_ACCESS_TOKEN'];
	if (!token && !warnedMissingToken) {
		log.fetchMaps.warn('MAPBOX_ACCESS_TOKEN not configured; map views disabled');
		warnedMissingToken = true;
	}
	return token ?? null;
}

/**
 * Generate deterministic hash from coordinates and zoom
 * Used for cache invalidation when location changes
 */
function hashCoordinates(lat: number, lng: number, zoom: number): string {
	const input = `${lat.toFixed(6)},${lng.toFixed(6)},${zoom}`;
	return new Bun.CryptoHasher('sha256').update(input).digest('hex').slice(0, 8);
}

/**
 * Generate map view filename
 * Format: {id}-{view}-{coordhash}.png
 */
export function generateMapFilename(id: string, view: MapViewType, lat: number, lng: number): string {
	return `${id}-${view}-${hashCoordinates(lat, lng, MAP_CONFIG.zoom)}.png`;
}

/**
 * Build Mapbox Static Images API URL with access token
 */
function buildMapUrl(view: MapViewType, lat: number, lng: number, accessToken: string): string {
	const style = MAP_STYLES[view];
	const { zoom, width, height, retina } = MAP_CONFIG;
	const retinaStr = retina ? '@2x' : '';
	const marker = `pin-l+f00(${lng},${lat})`;
	return `https://api.mapbox.com/styles/v1/${style}/static/${marker}/${lng},${lat},${zoom}/${width}x${height}${retinaStr}?access_token=${accessToken}`;
}

/**
 * Build public map URL (without access token)
 * Used for display and storage
 */
export function buildPublicMapUrl(view: MapViewType, lat: number, lng: number): string {
	const style = MAP_STYLES[view];
	const { zoom, width, height, retina } = MAP_CONFIG;
	const retinaStr = retina ? '@2x' : '';
	const marker = `pin-l+f00(${lng},${lat})`;
	return `https://api.mapbox.com/styles/v1/${style}/static/${marker}/${lng},${lat},${zoom}/${width}x${height}${retinaStr}`;
}

/**
 * Read response body with timeout protection
 */
async function readBodyWithTimeout(response: Response, timeoutMs: number, label: string): Promise<Buffer | null> {
	let timeoutId: Timer | undefined;
	const timeoutPromise = new Promise<null>((resolve) => {
		timeoutId = setTimeout(() => {
			log.fetchMaps.warn(`[${label}] Body read timeout after ${timeoutMs}ms`);
			resolve(null);
		}, timeoutMs);
	});
	const bodyPromise = response.arrayBuffer().then((ab) => Buffer.from(ab));
	const result = await Promise.race([bodyPromise, timeoutPromise]);
	if (timeoutId) clearTimeout(timeoutId);
	return result;
}

/** Check if a Mapbox response status should be retried */
function isRetryableStatus(status?: number): boolean {
	if (!status) return true;
	return status === 429 || status >= 500;
}

type FetchAttempt = { ok: true; buffer: Buffer } | { ok: false; status?: number; retryAfter?: string | null; error?: string };

/**
 * Attempt a single Mapbox fetch (no retries)
 */
async function attemptFetch(url: string, label: string): Promise<FetchAttempt> {
	const controller = new AbortController();
	const connectionTimeout = setTimeout(() => controller.abort(), MAP_FETCH_TIMEOUT_MS);
	try {
		const response = await fetch(url, {
			signal: controller.signal,
		});
		clearTimeout(connectionTimeout);

		if (!response.ok) {
			return { ok: false, status: response.status, retryAfter: response.headers.get('Retry-After') };
		}

		const buffer = await readBodyWithTimeout(response, MAP_FETCH_TIMEOUT_MS, label);
		if (!buffer) {
			return { ok: false, status: response.status, retryAfter: response.headers.get('Retry-After'), error: 'body-timeout' };
		}

		return { ok: true, buffer };
	} catch (e) {
		clearTimeout(connectionTimeout);
		const isTimeout = e instanceof Error && e.name === 'AbortError';
		return { ok: false, error: isTimeout ? 'timeout' : e instanceof Error ? e.message : 'unknown' };
	}
}

/**
 * Fetch a map image buffer with retries
 */
async function fetchMapBuffer(id: string, view: MapViewType, lat: number, lng: number, accessToken: string): Promise<Buffer | null> {
	const url = buildMapUrl(view, lat, lng, accessToken);
	for (let attempt = 1; attempt <= MAP_MAX_RETRIES; attempt++) {
		const result = await attemptFetch(url, `${view} map`);
		if (result.ok) return result.buffer;

		const retryable = isRetryableStatus(result.status);
		if (attempt < MAP_MAX_RETRIES && retryable) {
			const backoff = calculateBackoff(attempt, MAP_RETRY_DELAY_MS, result.retryAfter);
			log.fetchMaps.debug('Mapbox retry backoff', { id, view, attempt, backoff: Math.round(backoff) });
			await sleep(backoff);
			continue;
		}

		if (result.status !== undefined) {
			log.fetchMaps.warn('Mapbox fetch failed', { id, view, status: result.status });
		} else {
			log.fetchMaps.warn('Mapbox fetch error', { id, view, error: result.error ?? 'Unknown error' });
		}
		return null;
	}

	return null;
}

/**
 * Fetch and process a single map view with timeout
 */
async function fetchSingleView(id: string, view: MapViewType, lat: number, lng: number, listingDir: string, accessToken: string): Promise<MapViewEntry> {
	const filename = generateMapFilename(id, view, lat, lng);
	const outputPath = join(listingDir, filename);
	const publicUrl = buildPublicMapUrl(view, lat, lng);

	// Check cache first
	if (existsSync(outputPath)) {
		log.fetchMaps.info('Skipped map view (cached)', { id, view, filename });
		return { remote: publicUrl, local: filename };
	}

	// Fetch from Mapbox with retry + backoff
	log.fetchMaps.info('Fetching map view', { id, view, lat, lng });

	const buffer = await fetchMapBuffer(id, view, lat, lng, accessToken);
	if (!buffer) {
		return { remote: publicUrl, local: null };
	}

	try {
		await Bun.write(outputPath, buffer);
		log.fetchMaps.success('Map view cached', { id, view, filename, size: `${Math.round(buffer.length / 1024)}KB` });
		return { remote: publicUrl, local: filename };
	} catch (e) {
		log.fetchMaps.warn('Map view write failed', { id, view, error: e instanceof Error ? e.message : String(e) });
		return { remote: publicUrl, local: null };
	}
}

// =============================================================================
// PUBLIC API
// =============================================================================

/**
 * Fetch and cache both map views (satellite + street) for a property
 *
 * @param id - Listing ID
 * @param lat - Latitude
 * @param lng - Longitude
 * @param cacheDir - Root cache directory
 * @returns Result with both map view entries or error
 */
export async function fetchMapViews(id: string, lat: number, lng: number, cacheDir: string): Promise<MapViewsFetchResult> {
	const accessToken = getAccessToken();
	if (!accessToken) {
		return { success: false, error: 'No Mapbox token' };
	}

	// Ensure listing cache directory exists
	const listingDir = join(cacheDir, id);
	mkdirSync(listingDir, { recursive: true });

	// Fetch both views in parallel
	const [satellite, street] = await Promise.all([fetchSingleView(id, 'satellite', lat, lng, listingDir, accessToken), fetchSingleView(id, 'street', lat, lng, listingDir, accessToken)]);

	return {
		success: true,
		mapViews: { satellite, street },
	};
}

// =============================================================================
// BACKWARD COMPATIBILITY (deprecated, will be removed)
// =============================================================================

/** @deprecated Use MapViewEntry */
export type SatelliteEntry = MapViewEntry;

/** @deprecated Use MapViewsFetchResult */
export type SatelliteFetchResult = { success: true; satellite: MapViewEntry } | { success: false; error: string };

/** @deprecated Use generateMapFilename(id, 'satellite', lat, lng) */
export function generateSatelliteFilename(id: string, lat: number, lng: number): string {
	return generateMapFilename(id, 'satellite', lat, lng);
}

/** @deprecated Use buildPublicMapUrl('satellite', lat, lng) */
export function buildPublicSatelliteUrl(lat: number, lng: number): string {
	return buildPublicMapUrl('satellite', lat, lng);
}

/** @deprecated Use fetchMapViews */
export async function fetchSatelliteImage(id: string, lat: number, lng: number, cacheDir: string): Promise<SatelliteFetchResult> {
	const result = await fetchMapViews(id, lat, lng, cacheDir);
	if (!result.success) {
		return { success: false, error: result.error };
	}
	return { success: true, satellite: result.mapViews.satellite };
}
