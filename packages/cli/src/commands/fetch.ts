/**
 * Fetch commands - Unified data acquisition from Rightmove
 *
 * let fetch batch                - Batch fetch from config locations
 * let fetch id 123               - Fetch single listing by ID
 * let fetch id 123,456,789       - Fetch multiple specific IDs
 * let fetch location <name>      - Resolve region names to REGION IDs
 */

import type { Config } from '@let/core/config';
import { parseScoringConfig } from '@let/core/config';
import { closeAreaDbs, closeBroadbandDb } from '@let/core/pipeline/enrich';
import { buildSearchUrl, fetchWithRateLimit, lookupLocation, searchListingsApi, setApiDelay, setApiMaxRetries, setFetchDelay, setFetchMaxRetries } from '@let/core/pipeline/fetch';
import { scrapeSearchResults } from '@let/core/pipeline/parse';
import { buildScoringContext, recalcAssessedScores, scoreListingsWithConfig, scoreSingleListing } from '@let/core/pipeline/score';
import type { Listing, ListingsFile } from '@let/core/schema';
import { log } from '@let/core/utils/logger';
import { defineCommand } from 'citty';
import { downloadListingAssets, LISTINGS_DB_PATH, loadConfigOrExit, loadExistingListings, processListing, saveListingsFile } from './shared.js';
import { renderDetail } from './view/index.js';

/** Search filters type */
type SearchFilters = {
	minBedrooms: number;
	maxBedrooms: number;
	minPrice: number;
	maxPrice: number;
	propertyTypes: string[];
	includeLetAgreed: boolean;
	radius: number;
	dontShow: string[];
	mustHave: string[];
};

/** Search result from API or HTML scraping */
type LocationSearchResult = {
	listingIds: string[];
	totalResults: number;
	searchUrl?: string;
};

function getListingKey(listing: Listing): string {
	return listing.portalIds.rightmove ?? listing.id;
}

/** Result of processing a location */
type LocationProcessResult = {
	listings: Listing[];
	totalResults: number;
	searchUrl?: string | undefined;
};

/** Build search URL params from config filters */
function buildSearchParams(locationId: string, filters: SearchFilters): Parameters<typeof buildSearchUrl>[0] {
	return {
		locationIdentifier: locationId,
		minBedrooms: filters.minBedrooms,
		maxBedrooms: filters.maxBedrooms,
		minPrice: filters.minPrice,
		maxPrice: filters.maxPrice,
		propertyTypes: filters.propertyTypes,
		includeLetAgreed: filters.includeLetAgreed,
		radius: filters.radius,
		dontShow: filters.dontShow,
		mustHave: filters.mustHave,
	};
}

/** Build API search params from config filters */
function buildApiParams(locationId: string, filters: SearchFilters, index = 0): Parameters<typeof searchListingsApi>[0] {
	return {
		locationIdentifier: locationId,
		minBedrooms: filters.minBedrooms,
		maxBedrooms: filters.maxBedrooms,
		minPrice: filters.minPrice,
		maxPrice: filters.maxPrice,
		propertyTypes: filters.propertyTypes,
		includeLetAgreed: filters.includeLetAgreed,
		radius: filters.radius,
		dontShow: filters.dontShow,
		mustHave: filters.mustHave,
		index,
	};
}

/** Find the index of an existing listing matching the new one by portal ID or internal ID */
function findExistingIndex(existing: Listing[], newListing: Listing): number {
	const uprn = newListing.uprn;
	if (uprn) {
		const uprnIndex = existing.findIndex((l) => l.uprn === uprn);
		if (uprnIndex >= 0) return uprnIndex;
	}
	const rightmoveId = newListing.portalIds.rightmove;
	return rightmoveId ? existing.findIndex((l) => l.portalIds.rightmove === rightmoveId) : existing.findIndex((l) => l.id === newListing.id);
}

/** Preserve fields from an existing listing that the new listing lacks */
function preserveExistingFields(target: Listing, source: Listing): void {
	target.id = source.id;
	target.portalIds = { ...source.portalIds, ...target.portalIds };
	if (!target.notionPageId && source.notionPageId) target.notionPageId = source.notionPageId;
	if (!target.assessment && source.assessment) target.assessment = source.assessment;
	if (!target.assessedAt && source.assessedAt) target.assessedAt = source.assessedAt;
	if (!target.assessedScore && source.assessedScore) target.assessedScore = source.assessedScore;
}

/**
 * Merge a single listing into existing listings array (dedupe by ID)
 */
function mergeListings(existing: Listing[], newListing: Listing): Listing[] {
	const existingIndex = findExistingIndex(existing, newListing);
	if (existingIndex >= 0) {
		const existingListing = existing[existingIndex];
		if (existingListing) {
			preserveExistingFields(newListing, existingListing);
		}
		existing[existingIndex] = newListing;
		return existing;
	}
	return [...existing, newListing];
}

function dedupeIds(ids: string[]): string[] {
	const seen = new Set<string>();
	const unique: string[] = [];
	for (const id of ids) {
		if (!id) continue;
		if (seen.has(id)) continue;
		seen.add(id);
		unique.push(id);
	}
	return unique;
}

/** Search a location via API with pagination */
async function searchViaApi(locationId: string, locationName: string, filters: SearchFilters, maxListings: number, delayMs: number): Promise<LocationSearchResult | null> {
	const allIds: string[] = [];
	let index = 0;
	let totalResults = 0;
	const pageSize = 24;

	while (true) {
		log.fetch.info('Searching via API', { location: locationName, page: index / pageSize + 1 });
		const result = await searchListingsApi(buildApiParams(locationId, filters, index));

		if (!result.success) {
			if (index === 0) {
				log.fetch.warn('API search failed, falling back to HTML', { location: locationName, error: result.error });
				return null;
			}
			break;
		}

		totalResults = result.totalResults;
		allIds.push(...result.listingIds);

		if (allIds.length >= maxListings) {
			log.fetch.success('API search complete (limit reached)', { location: locationName, total: totalResults, fetched: maxListings });
			return { listingIds: allIds.slice(0, maxListings), totalResults };
		}
		if (allIds.length >= totalResults || result.listingIds.length < pageSize) {
			break;
		}

		index += pageSize;
		await new Promise((r) => setTimeout(r, delayMs));
	}

	log.fetch.success('API search complete', { location: locationName, total: totalResults, fetched: allIds.length });
	return { listingIds: allIds, totalResults };
}

/** Fetch and parse a single page of HTML search results */
async function fetchHtmlSearchPage(searchUrl: string): Promise<{ success: true; listingIds: string[]; totalResults: number } | { success: false; error: string }> {
	const fetchResult = await fetchWithRateLimit(searchUrl);
	if (!fetchResult.success) {
		return { success: false, error: fetchResult.error };
	}
	const parseResult = scrapeSearchResults(fetchResult.html);
	if (!parseResult.success) {
		return { success: false, error: parseResult.error };
	}
	return { success: true, listingIds: parseResult.listingIds, totalResults: parseResult.totalResults };
}

/** Search a location via HTML scraping with pagination */
async function searchViaHtml(locationId: string, locationName: string, filters: SearchFilters, maxListings: number, delayMs: number): Promise<LocationSearchResult | null> {
	const allIds: string[] = [];
	let index = 0;
	let totalResults = 0;
	let searchUrl = '';
	const pageSize = 24;

	while (true) {
		searchUrl = buildSearchUrl({ ...buildSearchParams(locationId, filters), index });
		log.fetch.info('Fetching search results (HTML)', { location: locationName, page: index / pageSize + 1 });

		const result = await fetchHtmlSearchPage(searchUrl);
		if (!result.success) {
			if (index === 0) {
				log.fetch.error('Search failed', { location: locationName, error: result.error });
				return null;
			}
			break;
		}

		totalResults = result.totalResults;
		allIds.push(...result.listingIds);

		if (allIds.length >= maxListings) {
			log.parse.success('Search results parsed (limit reached)', { location: locationName, total: totalResults, fetched: maxListings });
			return { listingIds: allIds.slice(0, maxListings), totalResults, searchUrl };
		}
		if (allIds.length >= totalResults || result.listingIds.length < pageSize) {
			break;
		}

		index += pageSize;
		await new Promise((r) => setTimeout(r, delayMs));
	}

	log.parse.success('Search results parsed', { location: locationName, total: totalResults, fetched: allIds.length });
	return { listingIds: allIds, totalResults, searchUrl };
}

/** Options for batch processing */
type BatchOptions = { skipImages?: boolean };

/** Process a batch of listing IDs into Listing objects */
async function processListingBatch(ids: string[], locationName: string, region: string, options?: BatchOptions): Promise<Listing[]> {
	const listings: Listing[] = [];

	for (let i = 0; i < ids.length; i++) {
		const id = ids[i];
		if (!id) continue;

		log.cli.info('Progress', { location: locationName, current: i + 1, total: ids.length });

		const listing = await processListing(id, { region, ...(options?.skipImages ? { skipImages: true } : {}) });
		if (listing) listings.push(listing);
	}

	return listings;
}

/** Process a single location and return new listings */
async function processLocation(
	location: { id: string; name: string },
	existingIds: Set<string>,
	seenIds: Set<string>,
	filters: SearchFilters,
	options: { useApi: boolean; delay: number; limit: number },
	batchOptions?: BatchOptions,
): Promise<LocationProcessResult | null> {
	log.cli.info('Searching location', { name: location.name, id: location.id });

	const searchResult = options.useApi ? await searchViaApi(location.id, location.name, filters, options.limit, options.delay) : null;
	const result = searchResult ?? (await searchViaHtml(location.id, location.name, filters, options.limit, options.delay));
	if (!result) return null;

	const uniqueIds = dedupeIds(result.listingIds);
	const duplicateCount = result.listingIds.length - uniqueIds.length;
	if (duplicateCount > 0) {
		log.cli.warn('Duplicate listing IDs in search results', { location: location.name, removed: duplicateCount });
	}

	const newIds: string[] = [];
	let skippedExisting = 0;
	let skippedSeen = 0;
	for (const id of uniqueIds) {
		if (existingIds.has(id)) {
			skippedExisting += 1;
			continue;
		}
		if (seenIds.has(id)) {
			skippedSeen += 1;
			continue;
		}
		seenIds.add(id);
		newIds.push(id);
	}

	if (skippedExisting > 0 || skippedSeen > 0) {
		log.cli.info('Skipping existing listings', {
			existing: skippedExisting,
			duplicate: skippedSeen,
			new: newIds.length,
		});
	}

	const idsToProcess = newIds.slice(0, options.limit);
	if (idsToProcess.length === 0) {
		log.cli.info('No new listings to process', { location: location.name });
		return { listings: [], totalResults: result.totalResults, searchUrl: result.searchUrl };
	}

	log.cli.info('Processing listings', { location: location.name, count: idsToProcess.length });
	const listings = await processListingBatch(idsToProcess, location.name, location.name, batchOptions);
	return { listings, totalResults: result.totalResults, searchUrl: result.searchUrl };
}

/** Filter final listings so min-score applies only to new listings after full re-score */
function filterFinalListings(listings: Listing[], existingIds: Set<string>, minScore: number | undefined): Listing[] {
	if (minScore === undefined) return listings;
	const filtered = listings.filter((listing) => existingIds.has(getListingKey(listing)) || (listing.scores?._overall ?? 0) >= minScore);
	const removed = listings.length - filtered.length;
	if (removed > 0) {
		log.cli.info('Filtered low-scoring new listings', { filtered: removed, minScore, remaining: filtered.length });
	}
	return filtered;
}

type FetchArgs = { limit: number; delay: number; minScore: number | undefined; useApi: boolean };

/** Parse and validate fetch command arguments */
function parseFetchArgs(args: Record<string, unknown>, config: Config): FetchArgs | null {
	const limitStr = (args['limit'] as string | undefined) ?? String(config.fetch.maxListings);
	const limit = Number.parseInt(limitStr, 10);
	const delayStr = (args['delay'] as string | undefined) ?? String(config.fetch.delayMs);
	const delay = Number.parseInt(delayStr, 10);

	if (Number.isNaN(limit) || limit < 1) {
		log.cli.error('Invalid --limit value', { value: limitStr, expected: 'positive integer' });
		return null;
	}
	if (Number.isNaN(delay) || delay < 0) {
		log.cli.error('Invalid --delay value', { value: delayStr, expected: 'non-negative integer' });
		return null;
	}

	const minScoreStr = args['min-score'] as string | undefined;
	const minScore = minScoreStr ? Number.parseInt(minScoreStr, 10) : 70;
	if (minScore !== undefined && (Number.isNaN(minScore) || minScore < 0 || minScore > 100)) {
		log.cli.error('Invalid --min-score value', { value: minScoreStr, expected: '0-100' });
		return null;
	}

	return {
		limit,
		delay,
		minScore,
		useApi: (args['api'] as boolean) || config.fetch.useApi,
	};
}

type ProcessAllResult = { newListings: Listing[]; totalResults: number; searchUrls: string[] };

/** Process all configured locations and collect new listings */
async function processAllLocations(
	locations: Config['search']['locations'],
	existingIds: Set<string>,
	seenIds: Set<string>,
	filters: SearchFilters,
	opts: { useApi: boolean; delay: number; limit: number },
	batchOptions?: BatchOptions,
): Promise<ProcessAllResult> {
	const newListings: Listing[] = [];
	let totalResults = 0;
	const searchUrls: string[] = [];

	for (const location of locations) {
		const result = await processLocation(location, existingIds, seenIds, filters, opts, batchOptions);
		if (!result) continue;
		if (result.searchUrl) searchUrls.push(result.searchUrl);
		totalResults += result.totalResults;
		newListings.push(...result.listings);
	}

	return { newListings, totalResults, searchUrls };
}

/** Fetch listing(s) by ID and save to database */
async function fetchListingsById(idList: string, force: boolean): Promise<void> {
	try {
		const ids = idList
			.split(',')
			.map((i) => i.trim())
			.filter(Boolean);

		if (ids.length === 0) {
			log.cli.error('No valid IDs provided');
			process.exit(1);
		}

		log.cli.info('Fetch listings', { count: ids.length, force });

		const config = await loadConfigOrExit();
		setFetchDelay(config.fetch.delayMs);
		setFetchMaxRetries(config.fetch.maxRetries);
		setApiDelay(config.fetch.delayMs);
		setApiMaxRetries(config.fetch.maxRetries);
		const existing = loadExistingListings({ allowEmptyOnError: force });
		const scoringConfig = parseScoringConfig({ scoring: config.scoring });
		const context = buildScoringContext(existing.listings, scoringConfig);

		const newListings: Listing[] = [];

		for (let i = 0; i < ids.length; i++) {
			const id = ids[i];
			if (!id) continue;

			if (ids.length > 1) {
				log.cli.info('Progress', { current: i + 1, total: ids.length });
			}

			const listing = await processListing(id, { refresh: force });
			if (!listing) {
				log.cli.warn('Failed to fetch listing', { id });
				continue;
			}

			// Score using existing dataset for percentile context
			const scored = scoreSingleListing(listing, context);
			listing.scores = scored.scores;
			log.score.info('Scored', { id, score: listing.scores._overall });

			newListings.push(listing);
		}

		if (newListings.length === 0) {
			log.cli.error('No listings fetched');
			process.exit(1);
		}

		// Merge into existing listings
		let mergedListings = [...existing.listings];
		for (const listing of newListings) {
			mergedListings = mergeListings(mergedListings, listing);
		}

		// Re-score ALL listings together for consistent percentiles (same as batch mode)
		const finalListings = mergedListings.length > 0 ? scoreListingsWithConfig(mergedListings, { scoring: config.scoring }) : mergedListings;

		// Recalculate assessedScore for assessed listings (percentiles may have shifted)
		recalcAssessedScores(finalListings);

		// Save (finalListings already sorted by score from scoreListingsWithConfig)
		const output: ListingsFile = {
			updatedAt: new Date().toISOString(),
			searchUrls: existing.searchUrls,
			locations: existing.locations,
			lastSearchTotal: existing.lastSearchTotal,
			listings: finalListings,
		};

		await saveListingsFile(output);
		log.cli.success('Saved', { path: LISTINGS_DB_PATH, total: mergedListings.length, added: newListings.length });

		// Show detail for single listing, summary for multiple
		if (newListings.length === 1 && newListings[0]) {
			renderDetail(newListings[0]);
		}
	} finally {
		closeBroadbandDb();
		closeAreaDbs();
	}
}

/** Batch mode: fetch, score, and save from config locations */
async function fetchBatchMode(config: Config, args: Record<string, unknown>): Promise<void> {
	try {
		const parsed = parseFetchArgs(args, config);
		if (!parsed) process.exit(1);

		const { limit, delay, minScore, useApi } = parsed;

		setFetchDelay(delay);
		setFetchMaxRetries(config.fetch.maxRetries);
		setApiDelay(delay);
		setApiMaxRetries(config.fetch.maxRetries);
		log.cli.info('Fetch batch', { limit, delay, api: useApi, minScore, maxRetries: config.fetch.maxRetries });
		log.cli.info('Config loaded', { locations: config.search.locations.map((l) => l.name) });

		const existing = loadExistingListings({ allowEmptyOnError: args['force'] as boolean });
		const existingIds = new Set(existing.listings.map((l) => getListingKey(l)));
		const seenIds = new Set(existingIds);
		log.cli.info('Existing listings loaded', { count: existing.listings.length });

		// PHASE 1: Fetch all listings WITHOUT images (defer to after scoring)
		const { newListings, totalResults, searchUrls } = await processAllLocations(
			config.search.locations,
			existingIds,
			seenIds,
			config.search.filters,
			{ useApi, delay, limit },
			{ skipImages: true },
		);

		// PHASE 2: Merge existing + new, then re-score ALL listings together
		let mergedListings = [...existing.listings];
		for (const listing of newListings) {
			mergedListings = mergeListings(mergedListings, listing);
		}
		log.cli.info('Merging listings', {
			existing: existing.listings.length,
			newFetched: newListings.length,
			total: mergedListings.length,
		});

		// Re-score ALL listings together for consistent percentiles across runs
		// This ensures batch mode vs ID mode produce identical scores for the same listing
		const rescoredListings = mergedListings.length > 0 ? scoreListingsWithConfig(mergedListings, { scoring: config.scoring }) : [];

		// Recalculate assessedScore for assessed listings (percentiles may have shifted)
		recalcAssessedScores(rescoredListings);

		if (rescoredListings.length === 0) {
			log.cli.warn('No listings to save');
			return;
		}

		// Apply min-score filter to NEW listings only, after final re-score
		const finalListings = filterFinalListings(rescoredListings, existingIds, minScore);
		const passingNewListings = finalListings.filter((listing) => !existingIds.has(getListingKey(listing)));

		// PHASE 3: Download images for passing NEW listings only
		if (passingNewListings.length > 0) {
			log.cli.info('Downloading images for passing listings', { count: passingNewListings.length });
			for (const listing of passingNewListings) {
				await downloadListingAssets(listing);
			}
		}

		log.cli.info('Finalized listings', {
			newPassing: passingNewListings.length,
			total: finalListings.length,
		});

		const output: ListingsFile = {
			updatedAt: new Date().toISOString(),
			searchUrls: [...new Set([...existing.searchUrls, ...searchUrls])],
			locations: [...new Set([...existing.locations, ...config.search.locations.map((l) => l.name)])],
			lastSearchTotal: totalResults,
			listings: finalListings,
		};

		await saveListingsFile(output);
		log.cli.success('Results saved', {
			path: LISTINGS_DB_PATH,
			count: finalListings.length,
			newFetched: newListings.length,
			newAdded: passingNewListings.length,
		});
	} finally {
		closeBroadbandDb();
		closeAreaDbs();
	}
}

/**
 * let fetch location <name> - Resolve region names to REGION identifiers
 */
export const locationCommand = defineCommand({
	meta: {
		name: 'location',
		description: 'Resolve region names to Rightmove REGION identifiers',
	},
	args: {
		names: {
			type: 'positional',
			description: 'Comma-separated region names',
			required: true,
		},
	},
	async run({ args }) {
		const regions = args.names
			.split(',')
			.map((r) => r.trim())
			.filter(Boolean);

		if (regions.length === 0) {
			log.cli.error('No valid region names provided');
			process.exit(1);
		}

		log.cli.info('Location lookup', { count: regions.length });

		for (const region of regions) {
			log.fetch.info('Looking up', { region });
			const result = await lookupLocation(region);

			if (!result.success) {
				log.fetch.error('Lookup failed', { region, error: result.error });
				continue;
			}

			if (result.locations.length === 0) {
				log.cli.warn('No results found', { region });
				continue;
			}

			// Show top 5 results
			log.cli.success(`Results for "${region}":`);
			const topResults = result.locations.slice(0, 5);
			for (const loc of topResults) {
				log.cli.info(`  ${loc.displayName} -> ${loc.locationIdentifier}`);
			}
		}

		log.cli.success('Location lookup complete');
	},
});

/**
 * let fetch batch - Batch fetch from config locations
 */
export const batchCommand = defineCommand({
	meta: {
		name: 'batch',
		description: 'Batch fetch from config locations',
	},
	args: {
		force: {
			type: 'boolean',
			description: 'Allow recovery if DB load fails',
			default: false,
		},
		limit: {
			type: 'string',
			description: 'Max new listings per region',
		},
		delay: {
			type: 'string',
			description: 'Delay between requests in ms (defaults to config)',
		},
		api: {
			type: 'boolean',
			description: 'Use REST API for search',
			default: false,
		},
		'min-score': {
			type: 'string',
			default: '70',
			description: 'Only add listings above this score (0-100, default: 70)',
		},
	},
	async run({ args }) {
		const config = await loadConfigOrExit();
		await fetchBatchMode(config, args);
	},
});

/**
 * let fetch id <ids> - Fetch listing(s) by ID
 */
export const idCommand = defineCommand({
	meta: {
		name: 'id',
		description: 'Fetch listing(s) by ID',
	},
	args: {
		ids: {
			type: 'positional',
			description: 'Listing ID(s), comma-separated',
			required: true,
		},
		force: {
			type: 'boolean',
			description: 'Re-fetch even if cached (also allows recovery if DB load fails)',
			default: false,
		},
	},
	async run({ args }) {
		await fetchListingsById(args.ids, args.force as boolean);
	},
});

/**
 * let fetch - Parent command (structural only, no run function)
 */
export const fetchCommand = defineCommand({
	meta: {
		name: 'fetch',
		description: 'Fetch listings from Rightmove',
	},
	subCommands: {
		batch: batchCommand,
		id: idCommand,
		location: locationCommand,
	},
});
