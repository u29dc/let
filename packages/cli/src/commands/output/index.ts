/**
 * Output commands - Sync listings to external services
 *
 * let output notion - Export listings to Notion database
 */

import { writeFileSync } from 'node:fs';
import { loadListingsFile } from '@let/core/db';
import { createNotionPage, updateNotionPage, validateDatabase } from '@let/core/pipeline/output';
import { truncate } from '@let/core/pipeline/view';
import type { Listing, ListingsFile } from '@let/core/schema';
import { log } from '@let/core/utils/logger';
import { defineCommand } from 'citty';
import { createTable, formatPrice, formatScoreWithSignal, formatValue, printKeyValues, section } from '../../output/index.js';
import { LISTINGS_DB_PATH, LISTINGS_JSON_PATH, loadExistingListings, saveListingsFile } from '../shared.js';

/** Get Notion config from environment */
function getNotionConfig(): { apiKey: string; databaseId: string } | null {
	const apiKey = process.env['NOTION_API_KEY'];
	const databaseId = process.env['NOTION_DATABASE_ID'];

	if (!apiKey) {
		log.cli.error('Missing NOTION_API_KEY environment variable');
		return null;
	}
	if (!databaseId) {
		log.cli.error('Missing NOTION_DATABASE_ID environment variable');
		return null;
	}

	return { apiKey, databaseId };
}

/** Filter and sort listings for output */
function prepareListings(listings: Listing[], args: { top?: string; minScore?: string; region?: string }): Listing[] {
	let filtered = [...listings];

	// Filter by region
	if (args.region) {
		const regionLower = args.region.toLowerCase();
		filtered = filtered.filter((l) => l.region?.toLowerCase().includes(regionLower));
		log.cli.info('Filtered by region', { region: args.region, count: filtered.length });
	}

	// Filter by minimum score
	if (args.minScore) {
		const minScore = Number.parseInt(args.minScore, 10);
		if (!Number.isNaN(minScore)) {
			filtered = filtered.filter((l) => (l.scores?._overall ?? 0) >= minScore);
			log.cli.info('Filtered by min score', { minScore, count: filtered.length });
		}
	}

	// Sort by score descending
	filtered.sort((a, b) => (b.scores?._overall ?? 0) - (a.scores?._overall ?? 0));

	// Limit to top N
	if (args.top) {
		const top = Number.parseInt(args.top, 10);
		if (!Number.isNaN(top) && top > 0) {
			filtered = filtered.slice(0, top);
			log.cli.info('Limited to top', { top, count: filtered.length });
		}
	}

	return filtered;
}

/** Display output preview as table with Notion column names */
function displayNotionPreview(listings: Listing[]): void {
	section('Notion Output Preview');

	const table = createTable([
		{ name: 'name', title: 'NAME', alignment: 'left' },
		{ name: 'price', title: 'PRICE', alignment: 'right' },
		{ name: 'beds', title: 'BEDS', alignment: 'right' },
		{ name: 'score', title: 'SCORE', alignment: 'right' },
		{ name: 'epc', title: 'EPC', alignment: 'center' },
		{ name: 'garden', title: 'GARDEN', alignment: 'left' },
		{ name: 'heating', title: 'HEAT', alignment: 'left' },
		{ name: 'pets', title: 'PETS', alignment: 'left' },
		{ name: 'region', title: 'REGION', alignment: 'left' },
		{ name: 'images', title: 'IMAGES', alignment: 'right' },
	]);

	for (const l of listings) {
		table.addRow({
			name: truncate(l.address, 35),
			price: formatPrice(l.price),
			beds: l.bedrooms,
			score: formatScoreWithSignal(l.scores?._overall ?? null),
			epc: formatValue(l.epcRating),
			garden: formatValue(l.scores?.factors?.gardenType),
			heating: formatValue(l.scores?.factors?.heatingType),
			pets: formatValue(l.scores?.factors?.petPolicy),
			region: truncate(l.region ?? 'Unknown', 15),
			images: `${l.images.length}`,
		});
	}

	table.printTable();
}

/** Output result tracking */
type OutputStats = { created: number; updated: number; skipped: number; failed: number };

/** Output a single listing to Notion */
async function outputSingleListing(listing: Listing, config: { apiKey: string; databaseId: string }, sync: boolean, stats: OutputStats, updatedListings: Listing[]): Promise<void> {
	if (listing.notionPageId) {
		if (sync) {
			try {
				await updateNotionPage(config, listing.notionPageId, listing);
				stats.updated++;
				updatedListings.push(listing);
			} catch (e) {
				// If page is archived/deleted, clear the ID and create new
				if (e instanceof Error && e.message.includes('archived')) {
					log.notion.warn('Page archived, creating new', { id: listing.id });
					listing.notionPageId = undefined;
					const pageId = await createNotionPage(config, listing);
					listing.notionPageId = pageId;
					stats.created++;
					updatedListings.push(listing);
				} else {
					throw e;
				}
			}
		} else {
			stats.skipped++;
		}
	} else {
		const pageId = await createNotionPage(config, listing);
		listing.notionPageId = pageId;
		stats.created++;
		updatedListings.push(listing);
	}
}

/** Save updated listings with notionPageId */
async function saveUpdatedListings(existing: { listings: Listing[]; searchUrls: string[]; locations: string[]; lastSearchTotal: number }, updatedListings: Listing[]): Promise<void> {
	const listingsMap = new Map(existing.listings.map((l) => [l.id, l]));
	for (const updated of updatedListings) {
		const original = listingsMap.get(updated.id);
		if (original) {
			original.notionPageId = updated.notionPageId;
		}
	}

	const output: ListingsFile = {
		updatedAt: new Date().toISOString(),
		searchUrls: existing.searchUrls,
		locations: existing.locations,
		lastSearchTotal: existing.lastSearchTotal,
		listings: existing.listings,
	};
	await saveListingsFile(output);
	log.cli.info('Updated listings database with Notion page IDs', { path: LISTINGS_DB_PATH });
}

/** Display output summary */
function displaySummary(stats: OutputStats): void {
	section('Output Summary');
	const rows: [string, string][] = [
		['Created', `${stats.created}`],
		['Updated', `${stats.updated}`],
		['Skipped', `${stats.skipped}`],
	];
	if (stats.failed > 0) rows.push(['Failed', `${stats.failed}`]);
	printKeyValues(rows, { keyWidth: 7 });
}

/**
 * let output notion - Export listings to Notion database
 */
const outputNotion = defineCommand({
	meta: {
		name: 'notion',
		description: 'Output listings to Notion database',
	},
	args: {
		top: {
			type: 'string',
			description: 'Output top N listings by score',
		},
		'min-score': {
			type: 'string',
			description: 'Minimum score threshold (0-100)',
		},
		region: {
			type: 'string',
			description: 'Filter by region name',
		},
		'dry-run': {
			type: 'boolean',
			description: 'Preview without creating pages',
			default: false,
		},
		force: {
			type: 'boolean',
			description: 'Update existing pages instead of skipping',
			default: false,
		},
	},
	async run({ args }) {
		// Get Notion credentials
		const config = getNotionConfig();
		if (!config) {
			process.exit(1);
		}

		// Validate database access
		const isValid = await validateDatabase(config);
		if (!isValid) {
			log.cli.error('Cannot access Notion database. Check your API key and database ID.');
			log.cli.info('Make sure the integration has access to the database in Notion settings.');
			process.exit(1);
		}

		// Load listings
		const existing = loadExistingListings();
		if (existing.listings.length === 0) {
			log.cli.warn('No listings to output');
			return;
		}

		// Filter and prepare
		const listings = prepareListings(existing.listings, {
			top: args.top,
			minScore: args['min-score'],
			region: args.region,
		});

		if (listings.length === 0) {
			log.cli.warn('No listings match the filter criteria');
			return;
		}

		// Preview
		displayNotionPreview(listings);

		if (args['dry-run']) {
			log.cli.info('Dry run - no changes made', { total: listings.length });
			return;
		}

		// Output to Notion
		const stats: OutputStats = { created: 0, updated: 0, skipped: 0, failed: 0 };
		const updatedListings: Listing[] = [];

		for (let i = 0; i < listings.length; i++) {
			const listing = listings[i];
			if (!listing) continue;

			log.cli.info('Progress', { current: i + 1, total: listings.length });

			try {
				await outputSingleListing(listing, config, args.force, stats, updatedListings);
			} catch (e) {
				const error = e instanceof Error ? e.message : String(e);
				log.notion.error('Failed to output listing', { id: listing.id, error });
				stats.failed++;
			}
		}

		// Save updated listings with notionPageId
		if (updatedListings.length > 0) {
			await saveUpdatedListings(existing, updatedListings);
		}

		displaySummary(stats);
	},
});

/**
 * let output json - Export listings database to JSON backup
 */
const outputJson = defineCommand({
	meta: {
		name: 'json',
		description: 'Output listings database to JSON backup',
	},
	args: {
		output: {
			type: 'string',
			description: 'Output path for JSON backup',
			default: LISTINGS_JSON_PATH,
		},
	},
	run({ args }) {
		const data = loadListingsFile(LISTINGS_DB_PATH);
		writeFileSync(args.output, JSON.stringify(data, null, '\t'));
		log.cli.success('JSON output saved', { path: args.output, listings: data.listings.length });
	},
});

/**
 * Main output command with subcommands
 */
export const outputCommand = defineCommand({
	meta: {
		name: 'output',
		description: 'Output listings to external services',
	},
	subCommands: {
		notion: outputNotion,
		json: outputJson,
	},
});
