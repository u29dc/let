/**
 * `export notion` — Export listings to Notion database.
 *
 * Dry-run supported; partial success with created/updated/failed counts.
 * Rate-limit safe via Notion API limits. Structured errors.
 */

import { loadListingsFile, saveListingsFile as saveToDb } from '@let/core/db';
import { paths } from '@let/core/paths';
import { createNotionPage, updateNotionPage, validateDatabase } from '@let/core/pipeline/output';
import { queryListings } from '@let/core/pipeline/view';
import type { Listing, ListingsFile } from '@let/core/schema';
import { log, setQuietMode } from '@let/core/utils/logger';
import { fail, isJsonMode, ok, rethrowCapture } from '../../envelope.js';
import { defineToolCommand } from '../../tool.js';

type NotionConfig = { apiKey: string; databaseId: string };

function getNotionConfig(): NotionConfig | null {
	const apiKey = process.env['NOTION_API_KEY'];
	const databaseId = process.env['NOTION_DATABASE_ID'];
	if (!apiKey || !databaseId) return null;
	return { apiKey, databaseId };
}

/** Validate Notion credentials or exit */
async function requireNotionConfig(jsonMode: boolean, start: number): Promise<NotionConfig> {
	const config = getNotionConfig();
	if (!config) {
		if (jsonMode) fail('export.notion', 'NO_CREDENTIALS', 'Missing NOTION_API_KEY or NOTION_DATABASE_ID', 'Set env vars', start);
		log.cli.error('Missing NOTION_API_KEY or NOTION_DATABASE_ID');
		process.exit(1);
	}

	const isValid = await validateDatabase(config);
	if (!isValid) {
		if (jsonMode) fail('export.notion', 'INVALID_DB', 'Cannot access Notion database', 'Check API key and database ID', start);
		log.cli.error('Cannot access Notion database');
		process.exit(1);
	}

	return config;
}

type OutputStats = { created: number; updated: number; skipped: number; failed: number };

async function outputSingleListing(listing: Listing, config: { apiKey: string; databaseId: string }, sync: boolean, stats: OutputStats): Promise<void> {
	if (listing.notionPageId) {
		if (sync) {
			await updateNotionPage(config, listing.notionPageId, listing);
			stats.updated++;
		} else {
			stats.skipped++;
		}
	} else {
		const pageId = await createNotionPage(config, listing);
		listing.notionPageId = pageId;
		stats.created++;
	}
}

async function exportListings(listings: Listing[], config: { apiKey: string; databaseId: string }, sync: boolean): Promise<OutputStats> {
	const stats: OutputStats = { created: 0, updated: 0, skipped: 0, failed: 0 };

	for (let i = 0; i < listings.length; i++) {
		const listing = listings[i];
		if (!listing) continue;

		log.cli.info('Progress', { current: i + 1, total: listings.length });

		try {
			await outputSingleListing(listing, config, sync, stats);
		} catch (e) {
			log.cli.warn('Failed to export', { id: listing.id, error: e instanceof Error ? e.message : String(e) });
			stats.failed++;
		}
	}

	return stats;
}

/** Parse filter args for listing selection */
function parseFilterArgs(args: { top?: string; 'min-score'?: string; region?: string }) {
	const top = args.top ? Number.parseInt(args.top, 10) : undefined;
	const minScore = args['min-score'] ? Number.parseInt(args['min-score'], 10) : undefined;
	return { top, minScore, region: args.region };
}

export const exportNotionCommand = defineToolCommand(
	{
		name: 'export.notion',
		command: 'let export notion',
		category: 'export',
		outputFields: ['created', 'updated', 'skipped', 'failed', 'total'],
		idempotent: false,
		rateLimit: '3 req/s (Notion API)',
		example: 'let export notion --top 20 --dry-run --json',
	},
	{
		meta: {
			name: 'notion',
			description: 'Export listings to Notion database',
		},
		args: {
			top: {
				type: 'string' as const,
				description: 'Export top N listings by score',
			},
			'min-score': {
				type: 'string' as const,
				description: 'Minimum score threshold',
			},
			region: {
				type: 'string' as const,
				description: 'Filter by region name',
			},
			'dry-run': {
				type: 'boolean' as const,
				description: 'Preview without exporting',
				default: false,
			},
			force: {
				type: 'boolean' as const,
				description: 'Update existing pages',
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
			if (jsonMode) setQuietMode(true);
			const p = paths();
			const emptyStats = { created: 0, updated: 0, skipped: 0, failed: 0, total: 0 };

			try {
				const config = await requireNotionConfig(jsonMode, start);
				const data = loadListingsFile(p.derived.database);
				const listings = data.listings ?? [];

				if (listings.length === 0) {
					if (jsonMode) ok('export.notion', emptyStats, start);
					log.cli.warn('No listings to export');
					return;
				}

				const filters = parseFilterArgs(args);
				const filtered = queryListings(listings, filters, 'score', true);

				if (args['dry-run']) {
					if (jsonMode) ok('export.notion', { ...emptyStats, total: filtered.length, dryRun: true }, start);
					log.cli.info('Dry run', { total: filtered.length });
					return;
				}

				const stats = await exportListings(filtered, config, args.force);

				saveToDb(p.derived.database, { ...data, updatedAt: new Date().toISOString(), listings } satisfies ListingsFile);

				if (jsonMode) ok('export.notion', { ...stats, total: filtered.length }, start);
				log.cli.success('Notion export complete', stats);
			} catch (error) {
				rethrowCapture(error);
				const message = error instanceof Error ? error.message : String(error);
				if (jsonMode) fail('export.notion', 'EXPORT_ERROR', `Export failed: ${message}`, 'Check credentials and network', start);
				log.cli.error(`Export failed: ${message}`);
				process.exit(1);
			}
		},
	},
);
