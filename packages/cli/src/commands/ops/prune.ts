/**
 * Ops command - prune low-scoring listings
 */

import * as readline from 'node:readline/promises';
import type { Listing, ListingsFile } from '@let/core/schema';
import { log } from '@let/core/utils/logger';
import { defineCommand } from 'citty';
import { isJsonMode, ok } from '../../envelope.js';
import { createTable, formatScoreWithSignal, printKeyValues, subheader } from '../../output/index.js';
import { loadExistingListings } from '../shared-read.js';
import { saveListingsFile } from '../shared-write.js';

/** Prompt user for confirmation */
async function promptConfirm(message: string): Promise<boolean> {
	const rl = readline.createInterface({ input: process.stdin, output: process.stdout });
	try {
		const answer = await rl.question(message);
		return answer.toLowerCase() === 'y';
	} finally {
		rl.close();
	}
}

/**
 * Quickselect algorithm - O(n) average time to find kth smallest element
 * Used for percentile cutoff calculation instead of O(n log n) full sort
 */
function quickselect<T>(arr: T[], k: number, compare: (a: T, b: T) => number): T | undefined {
	if (arr.length === 0) return undefined;
	if (arr.length === 1) return arr[0];
	if (k < 0 || k >= arr.length) return undefined;

	const pivot = arr[Math.floor(Math.random() * arr.length)];
	if (!pivot) return undefined;

	const lows: T[] = [];
	const highs: T[] = [];
	const pivots: T[] = [];

	for (const x of arr) {
		const cmp = compare(x, pivot);
		if (cmp < 0) lows.push(x);
		else if (cmp > 0) highs.push(x);
		else pivots.push(x);
	}

	if (k < lows.length) return quickselect(lows, k, compare);
	if (k < lows.length + pivots.length) return pivot;
	return quickselect(highs, k - lows.length - pivots.length, compare);
}

/** Display preview of listings to remove */
function displayRemovalPreview(listings: Listing[]): void {
	subheader('Listings to Remove');
	const table = createTable([
		{ name: 'id', title: 'ID', alignment: 'left' },
		{ name: 'score', title: 'SCORE', alignment: 'right' },
		{ name: 'address', title: 'ADDRESS', alignment: 'left' },
	]);
	for (const listing of listings.slice(0, 5)) {
		const displayId = listing.portalIds.rightmove ?? listing.id;
		table.addRow({
			id: displayId,
			score: formatScoreWithSignal(listing.scores?._overall ?? null),
			address: listing.address,
		});
	}
	table.printTable();
	if (listings.length > 5) {
		printKeyValues([['More', `${listings.length - 5}`]], { keyWidth: 4 });
	}
}

/** Existing listings data from loadExistingListings */
type ExistingListings = {
	listings: Listing[];
	searchUrls: string[];
	locations: string[];
	lastSearchTotal: number;
};

/** Args type for prune command */
type PruneArgs = {
	'dry-run': boolean;
	force: boolean;
	region?: string;
	inactive?: boolean;
	'min-score': string;
	bottom?: string;
};

/** Prune listings by region pattern match */
async function pruneByRegion(existing: ExistingListings, region: string, args: PruneArgs): Promise<void> {
	const regionPatterns = region
		.split(',')
		.map((r) => r.trim().toLowerCase())
		.filter(Boolean);

	const matchesRegion = (listingRegion: string | null | undefined): boolean => {
		if (!listingRegion) return false;
		const lower = listingRegion.toLowerCase();
		const city = lower.split(',')[0]?.trim() ?? lower;
		return regionPatterns.some((pattern) => city === pattern || city.startsWith(pattern) || lower === pattern);
	};

	const toRemove = existing.listings.filter((l) => matchesRegion(l.region));
	const toKeep = existing.listings.filter((l) => !matchesRegion(l.region));

	log.cli.info('Prune by region', { patterns: regionPatterns, removing: toRemove.length, keeping: toKeep.length });

	if (toRemove.length === 0) {
		log.cli.success('No listings found in specified regions');
		return;
	}

	displayRemovalPreview(toRemove);

	if (args['dry-run']) {
		log.cli.info('Dry run - no changes made');
		return;
	}

	if (!args.force) {
		const confirmed = await promptConfirm(`Remove ${toRemove.length} listings from ${regionPatterns.join(', ')}? (y/N) `);
		if (!confirmed) {
			log.cli.info('Aborted');
			return;
		}
	}

	const matchesLocationPattern = (loc: string): boolean => {
		const lower = loc.toLowerCase();
		const city = lower.split(',')[0]?.trim() ?? lower;
		return regionPatterns.some((pattern) => city === pattern || city.startsWith(pattern) || lower === pattern);
	};

	const output: ListingsFile = {
		updatedAt: new Date().toISOString(),
		searchUrls: existing.searchUrls,
		locations: existing.locations.filter((loc) => !matchesLocationPattern(loc)),
		lastSearchTotal: existing.lastSearchTotal,
		listings: toKeep,
	};
	await saveListingsFile(output);
	log.cli.success('Pruned by region', { removed: toRemove.length, remaining: toKeep.length });
}

/** Prune inactive listings */
async function pruneInactive(existing: ExistingListings, args: PruneArgs, jsonMode: boolean, start: number): Promise<void> {
	const toRemove = existing.listings.filter((l) => l.status === 'inactive');
	const toKeep = existing.listings.filter((l) => l.status !== 'inactive');

	if (toRemove.length === 0) {
		if (jsonMode) ok('ops.prune', { removed: 0, remaining: existing.listings.length, mode: 'inactive' }, start);
		log.cli.success('No inactive listings to prune');
		return;
	}

	if (!jsonMode) displayRemovalPreview(toRemove);

	if (!args['dry-run'] && (args.force || jsonMode)) {
		const output: ListingsFile = {
			updatedAt: new Date().toISOString(),
			searchUrls: existing.searchUrls,
			locations: existing.locations,
			lastSearchTotal: existing.lastSearchTotal,
			listings: toKeep,
		};
		await saveListingsFile(output);
	}

	if (jsonMode) ok('ops.prune', { removed: toRemove.length, remaining: toKeep.length, mode: 'inactive', dryRun: args['dry-run'] }, start);
	log.cli.success('Pruned inactive', { removed: toRemove.length, remaining: toKeep.length });
}

/** Prune listings by score threshold */
async function pruneByScore(existing: ExistingListings, args: PruneArgs): Promise<void> {
	let cutoff: number;
	let mode: string;

	if (args.bottom) {
		const pct = Number.parseInt(args.bottom, 10);
		if (Number.isNaN(pct) || pct < 1 || pct > 100) {
			log.cli.error('Invalid --bottom value', { value: args.bottom, expected: '1-100' });
			process.exit(1);
		}
		const cutoffIdx = Math.floor(existing.listings.length * (pct / 100));
		const cutoffListing = quickselect([...existing.listings], cutoffIdx, (a, b) => (a.scores?._overall ?? 0) - (b.scores?._overall ?? 0));
		cutoff = cutoffListing?.scores?._overall ?? 0;
		mode = `bottom ${pct}%`;
	} else {
		cutoff = Number.parseInt(args['min-score'], 10);
		if (Number.isNaN(cutoff) || cutoff < 0 || cutoff > 100) {
			log.cli.error('Invalid --min-score value', { value: args['min-score'], expected: '0-100' });
			process.exit(1);
		}
		mode = `score < ${cutoff}`;
	}

	const toRemove = existing.listings.filter((l) => (l.scores?._overall ?? 0) < cutoff);
	const toKeep = existing.listings.filter((l) => (l.scores?._overall ?? 0) >= cutoff);

	log.cli.info('Prune preview', { mode, removing: toRemove.length, keeping: toKeep.length });

	if (toRemove.length === 0) {
		log.cli.success('Nothing to prune');
		return;
	}

	displayRemovalPreview(toRemove);

	if (args['dry-run']) {
		log.cli.info('Dry run - no changes made');
		return;
	}

	if (!args.force) {
		const confirmed = await promptConfirm(`Remove ${toRemove.length} listings? (y/N) `);
		if (!confirmed) {
			log.cli.info('Aborted');
			return;
		}
	}

	const output: ListingsFile = {
		updatedAt: new Date().toISOString(),
		searchUrls: existing.searchUrls,
		locations: existing.locations,
		lastSearchTotal: existing.lastSearchTotal,
		listings: toKeep,
	};
	await saveListingsFile(output);
	log.cli.success('Pruned', { removed: toRemove.length, remaining: toKeep.length });
}

/**
 * let ops prune - Remove low-scoring listings from data file
 */
export const pruneCommand = defineCommand({
	meta: {
		name: 'prune',
		description: 'Remove low-scoring listings from data file',
	},
	args: {
		'min-score': {
			type: 'string',
			description: 'Remove listings below this score',
			default: '50',
		},
		bottom: {
			type: 'string',
			description: 'Remove bottom N% (overrides min-score)',
		},
		region: {
			type: 'string',
			description: 'Remove all listings from these regions (comma-separated)',
		},
		inactive: {
			type: 'boolean',
			description: 'Remove inactive listings',
			default: false,
		},
		'dry-run': {
			type: 'boolean',
			description: 'Preview without removing',
			default: false,
		},
		force: {
			type: 'boolean',
			description: 'Skip confirmation prompt',
			default: false,
		},
		json: {
			type: 'boolean',
			description: 'Output as JSON envelope',
			default: false,
		},
	},
	async run({ args }) {
		const start = performance.now();
		const jsonMode = isJsonMode();
		const existing = loadExistingListings();
		if (existing.listings.length === 0) {
			if (jsonMode) ok('ops.prune', { removed: 0, remaining: 0, mode: 'none' }, start);
			log.cli.warn('No listings to prune');
			return;
		}

		if (args.inactive) {
			await pruneInactive(existing, args, jsonMode, start);
			return;
		}

		if (args.region) {
			await pruneByRegion(existing, args.region, args);
		} else {
			await pruneByScore(existing, args);
		}
	},
});
