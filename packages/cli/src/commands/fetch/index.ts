/**
 * `fetch <ids>` — Atomic listing acquisition with partial success.
 *
 * Fetches, parses, enriches, scores, and persists listings by portal ID.
 * Returns fetched[] and failed[] arrays for partial success reporting.
 */

import { loadConfig, resetConfigCache } from '@let/core/config';
import { loadListingsFile, upsertListings } from '@let/core/db';
import { paths } from '@let/core/paths';
import { closeAreaDbs, closeBroadbandDb } from '@let/core/pipeline/enrich';
import { setApiDelay, setApiMaxRetries, setFetchDelay, setFetchMaxRetries } from '@let/core/pipeline/fetch';
import { recalcAssessedScores, scoreListingsWithConfig } from '@let/core/pipeline/score';
import type { Listing } from '@let/core/schema';
import { log } from '@let/core/utils/logger';
import { fail, isJsonMode, ok, rethrowCapture } from '../../envelope.js';
import { defineToolCommand } from '../../tool.js';
import { deduplicateListings, processListing } from '../shared-write.js';

type FetchedItem = { id: string; address: string; score: number | null };
type FailedItem = { id: string; error: string };

function loadExisting(dbPath: string) {
	try {
		const data = loadListingsFile(dbPath);
		return { listings: data.listings ?? [], searchUrls: data.searchUrls ?? [], locations: data.locations ?? [], lastSearchTotal: data.lastSearchTotal ?? 0 };
	} catch {
		return { listings: [] as Listing[], searchUrls: [] as string[], locations: [] as string[], lastSearchTotal: 0 };
	}
}

function parseInputIds(raw: string): string[] {
	return raw
		.split(',')
		.map((id: string) => id.trim())
		.filter(Boolean);
}

async function fetchListings(inputIds: string[], options: { skipImages: boolean; skipEpc: boolean; region?: string }) {
	const fetched: FetchedItem[] = [];
	const failed: FailedItem[] = [];
	const newListings: Listing[] = [];

	for (const id of inputIds) {
		try {
			const processOptions: Parameters<typeof processListing>[1] = { skipImages: options.skipImages, skipEpc: options.skipEpc };
			if (options.region) processOptions.region = options.region;
			const listing = await processListing(id, processOptions);
			if (listing) {
				newListings.push(listing);
				fetched.push({ id, address: listing.address, score: null });
			} else {
				failed.push({ id, error: 'Processing returned null' });
			}
		} catch (error) {
			failed.push({ id, error: error instanceof Error ? error.message : String(error) });
		}
	}

	return { fetched, failed, newListings };
}

function updateFetchedScores(fetched: FetchedItem[], scored: Listing[]) {
	for (const item of fetched) {
		const listing = scored.find((l) => l.portalIds.rightmove === item.id);
		if (listing) item.score = listing.scores?._overall ?? null;
	}
}

function validateInputIds(ids: string[], jsonMode: boolean, start: number): void {
	if (ids.length === 0) {
		if (jsonMode) {
			fail('fetch', 'VALIDATION_ERROR', 'No IDs provided', 'Provide comma-separated portal IDs', start);
		}
		log.cli.error('No IDs provided');
		process.exit(1);
	}
}

function cleanupDbs() {
	try {
		closeAreaDbs();
		closeBroadbandDb();
	} catch {
		/* ignore cleanup errors */
	}
}

export const fetchNewCommand = defineToolCommand(
	{
		name: 'fetch',
		command: 'let fetch',
		category: 'fetch',
		outputSchema: {
			fetched: { type: 'array', items: 'FetchedItem', description: 'Successful: { id, address, score }' },
			failed: { type: 'array', items: 'FailedItem', description: 'Failed: { id, error }' },
			total: { type: 'number', description: 'Total requested IDs' },
			saveError: { type: 'string', description: 'Optional: error message if DB save failed after fetch' },
		},
		idempotent: false,
		rateLimit: 'config fetch.delayMs per request',
		example: 'let fetch 170448131,170448132 --json',
	},
	{
		meta: {
			name: 'fetch',
			description: 'Fetch listings by portal ID',
		},
		args: {
			ids: {
				type: 'positional' as const,
				description: 'Comma-separated portal IDs to fetch',
				required: true,
			},
			region: {
				type: 'string' as const,
				description: 'Region name to assign to fetched listings',
			},
			'skip-images': {
				type: 'boolean' as const,
				description: 'Skip image/map downloads',
				default: false,
			},
			'skip-epc': {
				type: 'boolean' as const,
				description: 'Skip EPC API enrichment',
				default: false,
			},
			json: {
				type: 'boolean' as const,
				description: 'Output as JSON envelope',
				default: false,
			},
		},
		async run({ args }) {
			const start = performance.now();
			const jsonMode = isJsonMode();
			const p = paths();
			const dbPath = p.derived.database;
			const inputIds = parseInputIds(args.ids);

			validateInputIds(inputIds, jsonMode, start);

			try {
				resetConfigCache();
				const config = await loadConfig(p.derived.configFile);
				setFetchDelay(config.fetch.delayMs);
				setFetchMaxRetries(config.fetch.maxRetries);
				setApiDelay(config.fetch.delayMs);
				setApiMaxRetries(config.fetch.maxRetries);

				const existing = loadExisting(dbPath);
				const { fetched, failed, newListings } = await fetchListings(inputIds, { skipImages: args['skip-images'], skipEpc: args['skip-epc'], region: args.region });

				// Deduplicate: carry over persistent fields from existing listings to re-fetched ones
				const merged = [...existing.listings, ...newListings];
				const { uniqueListings, removed, replaced } = deduplicateListings(merged);
				if (removed > 0) {
					log.cli.warn('Duplicate listings resolved before save', { removed, replaced, remaining: uniqueListings.length });
				}

				const scored = scoreListingsWithConfig(uniqueListings, config as unknown as Record<string, unknown>);
				recalcAssessedScores(scored);
				updateFetchedScores(fetched, scored);

				// Partition scored listings into new inserts vs updated re-fetches
				const existingIds = new Set(existing.listings.map((l) => l.id));
				const trulyNew = scored.filter((l) => !existingIds.has(l.id));
				// Re-fetched listings have the same UUID (carried over by deduplicateListings)
				// but contain fresh data that must be written back
				const inputRightmoveIds = new Set(inputIds);
				const updated = scored.filter((l) => existingIds.has(l.id) && l.portalIds.rightmove != null && inputRightmoveIds.has(l.portalIds.rightmove));

				let saveError: string | undefined;
				try {
					upsertListings(dbPath, trulyNew, updated, scored, { updatedAt: new Date().toISOString(), lastSearchTotal: existing.lastSearchTotal }, existing.searchUrls, existing.locations);
				} catch (error) {
					saveError = error instanceof Error ? error.message : String(error);
					log.cli.error(`Save failed: ${saveError}`);
				}
				cleanupDbs();

				if (jsonMode) {
					const data: Record<string, unknown> = { fetched, failed, total: inputIds.length };
					if (saveError) data['saveError'] = saveError;
					ok('fetch', data, start);
				}

				log.cli.info(`Fetched ${fetched.length}/${inputIds.length} listings`);
				if (failed.length > 0) {
					log.cli.warn(`Failed: ${failed.map((f) => f.id).join(', ')}`);
				}
			} catch (error) {
				rethrowCapture(error);
				const message = error instanceof Error ? error.message : String(error);
				if (jsonMode) {
					fail('fetch', 'FETCH_ERROR', `Fetch failed: ${message}`, 'Check config and network', start);
				}
				log.cli.error(`Fetch failed: ${message}`);
				process.exit(1);
			}
		},
	},
);
