/**
 * `fetch <ids>` — Atomic listing acquisition with partial success.
 *
 * Fetches, parses, enriches, scores, and persists listings by portal ID.
 * Returns fetched[] and failed[] arrays for partial success reporting.
 */

import { loadConfig, resetConfigCache } from '@let/core/config';
import { loadListingsFile, saveListingsFile as saveToDb } from '@let/core/db';
import { paths } from '@let/core/paths';
import { closeAreaDbs, closeBroadbandDb } from '@let/core/pipeline/enrich';
import { setApiDelay, setApiMaxRetries, setFetchDelay, setFetchMaxRetries } from '@let/core/pipeline/fetch';
import { recalcAssessedScores, scoreListingsWithConfig } from '@let/core/pipeline/score';
import type { Listing, ListingsFile } from '@let/core/schema';
import { log } from '@let/core/utils/logger';
import { fail, isJsonMode, ok } from '../../envelope.js';
import { defineToolCommand } from '../../tool.js';
import { processListing } from '../shared-write.js';

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

async function fetchListings(inputIds: string[], options: { skipImages: boolean; skipEpc: boolean }) {
	const fetched: FetchedItem[] = [];
	const failed: FailedItem[] = [];
	const newListings: Listing[] = [];

	for (const id of inputIds) {
		try {
			const listing = await processListing(id, { skipImages: options.skipImages, skipEpc: options.skipEpc });
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
		outputFields: ['fetched', 'failed', 'total'],
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

			if (inputIds.length === 0) {
				if (jsonMode) {
					fail('fetch', 'VALIDATION_ERROR', 'No IDs provided', 'Provide comma-separated portal IDs', start);
				}
				log.cli.error('No IDs provided');
				process.exit(1);
			}

			try {
				resetConfigCache();
				const config = await loadConfig(p.derived.configFile);
				setFetchDelay(config.fetch.delayMs);
				setFetchMaxRetries(config.fetch.maxRetries);
				setApiDelay(config.fetch.delayMs);
				setApiMaxRetries(config.fetch.maxRetries);

				const existing = loadExisting(dbPath);
				const { fetched, failed, newListings } = await fetchListings(inputIds, { skipImages: args['skip-images'], skipEpc: args['skip-epc'] });

				const merged = [...existing.listings, ...newListings];
				const scored = scoreListingsWithConfig(merged, config as unknown as Record<string, unknown>);
				recalcAssessedScores(scored);
				updateFetchedScores(fetched, scored);

				const output: ListingsFile = {
					updatedAt: new Date().toISOString(),
					searchUrls: existing.searchUrls,
					locations: existing.locations,
					lastSearchTotal: existing.lastSearchTotal,
					listings: scored,
				};
				saveToDb(dbPath, output);
				cleanupDbs();

				if (jsonMode) {
					ok('fetch', { fetched, failed, total: inputIds.length }, start);
				}

				log.cli.info(`Fetched ${fetched.length}/${inputIds.length} listings`);
				if (failed.length > 0) {
					log.cli.warn(`Failed: ${failed.map((f) => f.id).join(', ')}`);
				}
			} catch (error) {
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
