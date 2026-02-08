/**
 * Shared write utilities for CLI commands
 * Heavy imports - scraper, EPC, broadband, config loading
 */

import { existsSync, mkdirSync, readFileSync, statSync, writeFileSync } from 'node:fs';
import { join } from 'node:path';
import { type Config, loadConfig } from '@let/core/config';
import { saveListingsFile as saveListingsToDb } from '@let/core/db';
import { paths } from '@let/core/paths';
import { applyEpcToListing, enrichListingArea, enrichListingNotes, enrichWithEpc, lookupBroadband } from '@let/core/pipeline/enrich';
import { buildListingUrl, downloadListingImages, fetchMapViews, fetchWithRateLimit } from '@let/core/pipeline/fetch';
import { extractPageModel, scrapeListing } from '@let/core/pipeline/parse';
import type { Listing, ListingsFile } from '@let/core/schema';
import { log } from '@let/core/utils/logger';

const CACHE_TTL_MS = 24 * 60 * 60 * 1000;

/** Check if cache file is stale (older than TTL). Returns true if stale or on error. */
function isCacheStale(path: string, id: string): boolean {
	try {
		const ageMs = Date.now() - statSync(path).mtimeMs;
		if (ageMs > CACHE_TTL_MS) {
			log.cli.debug('Cache expired, will refetch', { id, ageHours: Math.round(ageMs / 3600000) });
			return true;
		}
		return false;
	} catch (error) {
		log.cli.warn('Failed to read cache metadata, ignoring cache', { id, error: String(error) });
		return true;
	}
}

/** Read cache file and wrap as minimal HTML for extractPageModel. */
function readCacheAsHtml(path: string, id: string, label: string): string {
	log.cli.debug(`Using cached PAGE_MODEL${label}`, { id });
	const pageModel = readFileSync(path, 'utf-8');
	return `<script>window.PAGE_MODEL = ${pageModel}</script>`;
}

/**
 * Get cached data for a listing ID
 * Returns reconstructed HTML from cached PAGE_MODEL JSON
 * Cache structure: cache/{id}/data.json
 */
export function getCachedHtml(id: string, options: { allowStale?: boolean } = {}): string | undefined {
	const cacheDir = paths().resolved.cache;
	const newJsonPath = join(cacheDir, id, 'data.json');
	if (existsSync(newJsonPath)) {
		if (!options.allowStale && isCacheStale(newJsonPath, id)) return undefined;
		return readCacheAsHtml(newJsonPath, id, '');
	}
	const legacyJsonPath = join(cacheDir, `${id}.json`);
	if (existsSync(legacyJsonPath)) {
		if (!options.allowStale && isCacheStale(legacyJsonPath, id)) return undefined;
		return readCacheAsHtml(legacyJsonPath, id, ' (legacy)');
	}
	return undefined;
}

/**
 * Cache PAGE_MODEL JSON for a listing ID
 * Extracts the JSON data before caching to preserve it for later parsing
 * Cache structure: cache/{id}/data.json
 */
export function cachePageModel(id: string, html: string): void {
	const listingDir = paths().derived.cacheDir(id);
	mkdirSync(listingDir, { recursive: true });
	const jsonPath = join(listingDir, 'data.json');

	// Extract PAGE_MODEL JSON from HTML
	const result = extractPageModel(html);
	if (!result.success) {
		log.cli.warn('Could not extract PAGE_MODEL for caching', { id, error: result.error });
		return;
	}

	// Save as compact JSON
	const json = JSON.stringify(result.data);
	writeFileSync(jsonPath, json);
	log.cli.debug('Cached PAGE_MODEL', { id, size: `${Math.round(json.length / 1024)}KB` });
}

/** Options for processing a listing */
export type ProcessListingOptions = {
	dev?: boolean;
	refresh?: boolean;
	skipEpc?: boolean;
	skipBroadband?: boolean;
	skipArea?: boolean;
	skipImages?: boolean;
	/** Rightmove search region from config (e.g. "Manchester, Greater Manchester") */
	region?: string;
};

/** Enrich listing with EPC data (energy rating, floor area) */
async function enrichWithEpcData(listing: Listing, id: string): Promise<void> {
	if (!listing.postcode) return;
	const epcResult = await enrichWithEpc(listing);
	if (applyEpcToListing(listing, epcResult)) {
		log.enrich.success('EPC enriched', { id, rating: listing.epcRating, area: listing.floorAreaSqm });
	}
}

/** Enrich listing with broadband gigabit availability */
function enrichWithBroadbandData(listing: Listing, id: string): void {
	if (!listing.postcode) return;
	const broadbandResult = lookupBroadband(listing.postcode);
	if (broadbandResult) {
		listing.gigabitAvailability = broadbandResult.gigabitAvailability;
		log.enrich.debug('Broadband enriched', { id, postcode: listing.postcode, gigabit: broadbandResult.gigabitAvailability, source: broadbandResult.source });
	}
}

/** Enrich listing with area-level metrics (deprivation, crime, etc.) */
function enrichWithAreaData(listing: Listing, id: string): void {
	if (!listing.postcode) return;
	const areaResult = enrichListingArea(listing);
	if (areaResult.applied) {
		log.enrich.debug('Area metrics enriched', { id, postcode: listing.postcode, lsoa: listing.area.lsoa.code });
	}
}

/** Extract structured notes from listing description */
function enrichWithNotes(listing: Listing, id: string): void {
	if (!listing.description) return;
	const notesResult = enrichListingNotes(listing);
	if (notesResult.success && notesResult.notes.length > 0) {
		listing.notes = notesResult.notes;
		log.enrich.debug('Notes extracted', { id, count: notesResult.notes.length });
	}
}

/**
 * Apply enrichments to listing (EPC + broadband)
 */
async function enrichListing(listing: Listing, id: string, options: Pick<ProcessListingOptions, 'skipEpc' | 'skipBroadband' | 'skipArea' | 'dev'>): Promise<void> {
	if (!options.skipEpc && !options.dev) await enrichWithEpcData(listing, id);
	if (!options.skipBroadband) enrichWithBroadbandData(listing, id);
	if (!options.skipArea) enrichWithAreaData(listing, id);
	enrichWithNotes(listing, id);
}

/**
 * Get HTML for a listing (from cache or fetch)
 */
async function getListingHtml(id: string, options: Pick<ProcessListingOptions, 'refresh' | 'dev'>): Promise<string | null> {
	let html = options.refresh ? undefined : getCachedHtml(id, { allowStale: options.dev ?? false });
	if (options.dev && !html) {
		log.parse.error('No cached HTML in dev mode', { id });
		return null;
	}
	if (!html) {
		log.fetch.info('Fetching from Rightmove', { id });
		const result = await fetchWithRateLimit(buildListingUrl(id));
		if (!result.success) {
			log.fetch.error('Fetch failed', { id, error: result.error });
			return null;
		}
		html = result.html;
		cachePageModel(id, html);
	}
	return html;
}

/**
 * Download images and map views for a listing
 */
async function applyMediaAssets(listing: Listing, id: string, options: Pick<ProcessListingOptions, 'skipImages' | 'dev'>): Promise<void> {
	const cacheDir = paths().resolved.cache;
	const skip = options.skipImages || options.dev;
	if (!skip) {
		const imageResult = await downloadListingImages(id, listing.images, listing.floorplan, listing.epc, cacheDir);
		listing.images = imageResult.images;
		listing.floorplan = imageResult.floorplan;
		listing.epc = imageResult.epc;
	}
	if (!skip) {
		const mapResult = await fetchMapViews(id, listing.location.lat, listing.location.lng, cacheDir);
		listing.mapViews = mapResult.success ? mapResult.mapViews : { satellite: { remote: null, local: null }, street: { remote: null, local: null } };
	} else {
		listing.mapViews = { satellite: { remote: null, local: null }, street: { remote: null, local: null } };
	}
}

/**
 * Process a single listing
 */
export async function processListing(id: string, options: ProcessListingOptions): Promise<Listing | null> {
	log.parse.info('Processing listing', { id });

	const html = await getListingHtml(id, options);
	if (!html) return null;

	const result = await scrapeListing(id, html);
	if (!result.success) {
		log.parse.error('Parse failed', { id, error: result.error });
		return null;
	}

	const { listing } = result;
	if (options.region) listing.region = options.region;

	await enrichListing(listing, id, options);

	const shouldSkipImages = options.skipImages || options.dev;
	await applyMediaAssets(listing, id, { dev: options.dev ?? false, skipImages: shouldSkipImages ?? false });

	log.parse.success('Extracted listing', {
		id,
		address: listing.address,
		price: listing.price,
		bedrooms: listing.bedrooms,
		region: listing.region ?? 'Unknown',
	});

	return listing;
}

/**
 * Load and return config, or exit on error
 */
export async function loadConfigOrExit(): Promise<Config> {
	const configFile = paths().derived.configFile;
	try {
		return await loadConfig(configFile);
	} catch (e) {
		log.cli.error('Failed to load config', {
			path: configFile,
			error: e instanceof Error ? e.message : String(e),
		});
		process.exit(1);
	}
}

/** Parse a date string to a timestamp, returning 0 for invalid/missing values */
function toTimestamp(value: string | null | undefined): number {
	if (!value) return 0;
	const ts = Date.parse(value);
	return Number.isNaN(ts) ? 0 : ts;
}

/** Carry over persistent fields (notion, assessment) from an existing listing to the incoming one */
function carryOverPersistentFields(incoming: Listing, existing: Listing): void {
	const sameRightmoveId = existing.portalIds.rightmove && incoming.portalIds.rightmove === existing.portalIds.rightmove;
	if (!sameRightmoveId) return;

	incoming.id = existing.id;
	incoming.portalIds = { ...existing.portalIds, ...incoming.portalIds };
	if (!incoming.notionPageId && existing.notionPageId) incoming.notionPageId = existing.notionPageId;
	if (!incoming.assessment && existing.assessment) incoming.assessment = existing.assessment;
	if (!incoming.assessedAt && existing.assessedAt) incoming.assessedAt = existing.assessedAt;
	if (!incoming.assessedScore && existing.assessedScore) incoming.assessedScore = existing.assessedScore;
}

/** Deduplicate listings by rightmove portal ID, keeping the most recently fetched version */
export function deduplicateListings(listings: Listing[]): { uniqueListings: Listing[]; removed: number; replaced: number } {
	const indexById = new Map<string, number>();
	const uniqueListings: Listing[] = [];
	let removed = 0;
	let replaced = 0;

	for (const listing of listings) {
		const dedupeKey = listing.portalIds.rightmove ?? listing.id;
		const existingIndex = indexById.get(dedupeKey);

		if (existingIndex === undefined) {
			indexById.set(dedupeKey, uniqueListings.length);
			uniqueListings.push(listing);
			continue;
		}

		removed += 1;
		const existing = uniqueListings[existingIndex];
		if (!existing) continue;

		carryOverPersistentFields(listing, existing);

		const isNewer = toTimestamp(listing.fetchedAt) > toTimestamp(existing.fetchedAt);
		if (isNewer) {
			uniqueListings[existingIndex] = listing;
			replaced += 1;
		}
	}

	return { uniqueListings, removed, replaced };
}

/**
 * Save listings to SQLite database
 */
export async function saveListingsFile(output: ListingsFile): Promise<void> {
	const { uniqueListings, removed, replaced } = deduplicateListings(output.listings);

	if (removed > 0) {
		log.cli.warn('Duplicate listings resolved before save', { removed, replaced, remaining: uniqueListings.length });
	}

	saveListingsToDb(paths().derived.database, { ...output, listings: uniqueListings });
}

/**
 * Download images and maps for a listing (for deferred batch downloads)
 */
export async function downloadListingAssets(listing: Listing): Promise<void> {
	const cacheDir = paths().resolved.cache;
	const id = listing.portalIds.rightmove ?? listing.id;

	// Download images
	const imageResult = await downloadListingImages(id, listing.images, listing.floorplan, listing.epc, cacheDir);
	listing.images = imageResult.images;
	listing.floorplan = imageResult.floorplan;
	listing.epc = imageResult.epc;

	// Fetch map views
	const mapResult = await fetchMapViews(id, listing.location.lat, listing.location.lng, cacheDir);
	listing.mapViews = mapResult.success ? mapResult.mapViews : { satellite: { remote: null, local: null }, street: { remote: null, local: null } };
}
