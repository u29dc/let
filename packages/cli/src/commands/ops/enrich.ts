/**
 * Ops command - one-time backfill for area metrics
 */

import { enrichListingArea } from '@let/core/pipeline/enrich';
import type { Listing, ListingsFile } from '@let/core/schema';
import { log } from '@let/core/utils/logger';
import { defineCommand } from 'citty';
import { LISTINGS_DB_PATH, loadExistingListings, saveListingsFile } from '../shared.js';

function parseLimit(limitArg?: string): number | undefined {
	if (!limitArg) return undefined;
	const parsed = Number.parseInt(limitArg, 10);
	if (Number.isNaN(parsed) || parsed < 1) return undefined;
	return parsed;
}

async function runEnrichment(listings: Listing[], limit?: number): Promise<{ updated: number }> {
	let updated = 0;
	const toProcess = limit ? listings.slice(0, limit) : listings;

	for (let i = 0; i < toProcess.length; i++) {
		const listing = toProcess[i];
		if (!listing) continue;
		const displayId = listing.portalIds.rightmove ?? listing.id;
		log.cli.info('Enriching listing', { id: displayId, progress: `${i + 1}/${toProcess.length}` });

		const result = enrichListingArea(listing);
		if (result.applied) updated += 1;
	}

	return { updated };
}

export const enrichCommand = defineCommand({
	meta: {
		name: 'enrich',
		description: 'Backfill area metrics for existing listings (one-time)',
	},
	args: {
		limit: { type: 'string', description: 'Limit number of listings to process' },
		'dry-run': { type: 'boolean', description: 'Preview without saving', default: false },
	},
	async run({ args }) {
		const limit = parseLimit(args.limit as string | undefined);
		const existing = loadExistingListings();

		if (existing.listings.length === 0) {
			log.cli.warn('No listings to enrich');
			return;
		}

		log.cli.info('Backfill area metrics', { total: existing.listings.length, limit: limit ?? 'all', dryRun: args['dry-run'] });

		const result = await runEnrichment(existing.listings, limit);
		log.cli.info('Enrichment summary', { updated: result.updated });

		if (args['dry-run']) {
			log.cli.info('Dry run - no changes saved');
			return;
		}

		const output: ListingsFile = {
			updatedAt: new Date().toISOString(),
			searchUrls: existing.searchUrls,
			locations: existing.locations,
			lastSearchTotal: existing.lastSearchTotal,
			listings: existing.listings,
		};

		await saveListingsFile(output);
		log.cli.success('Backfill complete', { path: LISTINGS_DB_PATH, updated: result.updated });
	},
});
