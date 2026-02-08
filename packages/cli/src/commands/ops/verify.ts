/**
 * Ops command - verify listing availability
 */

import { paths } from '@let/core/paths';
import { buildListingUrl, fetchWithRateLimit, setFetchDelay } from '@let/core/pipeline/fetch';
import type { Listing, ListingsFile } from '@let/core/schema';
import { log } from '@let/core/utils/logger';
import { defineCommand } from 'citty';
import { isJsonMode, ok } from '../../envelope.js';
import { printKeyValues, section } from '../../output/index.js';
import { loadExistingListings } from '../shared-read.js';
import { saveListingsFile } from '../shared-write.js';

/** Region pattern matcher for filtering */
function matchesRegionPattern(listingRegion: string | null | undefined, patterns: string[]): boolean {
	if (!listingRegion) return false;
	const lower = listingRegion.toLowerCase();
	const city = lower.split(',')[0]?.trim() ?? lower;
	return patterns.some((pattern) => city === pattern || city.startsWith(pattern) || lower === pattern);
}

/** Parse region patterns from comma-separated string */
function parseRegionPatterns(regionArg: string | undefined): string[] {
	if (!regionArg) return [];
	return regionArg
		.split(',')
		.map((r) => r.trim().toLowerCase())
		.filter(Boolean);
}

/** Listing status from verification */
type VerifyStatus = 'active' | 'inactive';

/** Result of verifying a single listing */
type VerifyResult = {
	id: string;
	rightmoveId: string | null;
	status: VerifyStatus;
	error?: string;
};

/** Detect listing status from page HTML */
function detectListingStatus(html: string): VerifyStatus {
	const lower = html.toLowerCase();

	// Check for "Let Agreed" or removed/unavailable indicators
	if (
		lower.includes('let agreed') ||
		lower.includes('letagreed') ||
		lower.includes('no longer on the market') ||
		lower.includes('no longer available') ||
		lower.includes('this property has been removed')
	) {
		return 'inactive';
	}

	return 'active';
}

/** Verify a single listing's status */
async function verifyListingStatus(listing: Listing): Promise<VerifyResult> {
	const rightmoveId = listing.portalIds.rightmove ?? null;
	if (!rightmoveId) {
		return { id: listing.id, rightmoveId, status: 'active', error: 'Missing Rightmove ID' };
	}
	const url = buildListingUrl(rightmoveId);
	const result = await fetchWithRateLimit(url);

	if (!result.success) {
		// 404 or fetch error likely means inactive (removed)
		if (result.error.includes('404') || result.error.includes('Not Found')) {
			return { id: listing.id, rightmoveId, status: 'inactive' };
		}
		return { id: listing.id, rightmoveId, status: 'active', error: result.error };
	}

	const status = detectListingStatus(result.html);
	return { id: listing.id, rightmoveId, status };
}

/** Args for verify command */
type VerifyArgs = { dryRun: boolean; region: string | undefined; limit: number | undefined; delay: number };

/** Parse verify command arguments */
function parseVerifyArgs(args: Record<string, unknown>): VerifyArgs | null {
	const dryRun = args['dry-run'] as boolean;
	const regionArg = args['region'] as string | undefined;
	const limitArg = args['limit'] as string | undefined;
	const delayArg = args['delay'] as string;

	const delay = Number.parseInt(delayArg, 10);
	if (Number.isNaN(delay) || delay < 0) {
		log.cli.error('Invalid --delay value', { value: delayArg, expected: 'non-negative integer' });
		return null;
	}

	let limit: number | undefined;
	if (limitArg) {
		limit = Number.parseInt(limitArg, 10);
		if (Number.isNaN(limit) || limit < 1) {
			log.cli.error('Invalid --limit value', { value: limitArg, expected: 'positive integer' });
			return null;
		}
	}

	return { dryRun, region: regionArg, limit, delay };
}

/** Filter listings for verification */
function filterListingsForVerify(listings: Listing[], region?: string, limit?: number): Listing[] {
	let toVerify = listings;

	if (region) {
		const regionPatterns = parseRegionPatterns(region);
		toVerify = toVerify.filter((l) => matchesRegionPattern(l.region, regionPatterns));
		log.cli.info('Filtered by region', { patterns: regionPatterns, count: toVerify.length });
	}

	toVerify = toVerify.filter((l) => !l.status || l.status === 'active');

	if (limit) {
		toVerify = toVerify.slice(0, limit);
	}

	return toVerify;
}

/** Verify listings and collect results */
async function runVerification(toVerify: Listing[]): Promise<{ results: VerifyResult[]; inactive: number; errors: number }> {
	const results: VerifyResult[] = [];
	let inactive = 0;
	let errors = 0;

	for (let i = 0; i < toVerify.length; i++) {
		const listing = toVerify[i];
		if (!listing) continue;

		const displayId = listing.portalIds.rightmove ?? listing.id;
		log.cli.info('Checking', { id: displayId, progress: `${i + 1}/${toVerify.length}` });

		const result = await verifyListingStatus(listing);
		results.push(result);

		if (result.error) {
			log.cli.warn('Verify error', { id: displayId, error: result.error });
			errors++;
		} else if (result.status === 'inactive') {
			log.cli.info('Inactive', { id: displayId, address: listing.address });
			inactive++;
		}
	}

	return { results, inactive, errors };
}

/** Apply inactive status to listings based on verification results */
function applyInactiveStatus(listings: Listing[], results: VerifyResult[]): void {
	const listingsById = new Map(listings.map((l) => [l.id, l]));
	for (const result of results) {
		if (!result.error && result.status === 'inactive') {
			const listing = listingsById.get(result.id);
			if (listing) listing.status = 'inactive';
		}
	}
}

/** Print verification summary */
function printVerifySummary(total: number, inactive: number, errors: number): void {
	section('Verification Summary');
	const rows: [string, string][] = [
		['Total', `${total}`],
		['Active', `${total - inactive - errors}`],
		['Inactive', `${inactive}`],
	];
	if (errors > 0) rows.push(['Errors', `${errors}`]);
	printKeyValues(rows, { keyWidth: 7 });
}

/**
 * let ops verify - Check if listings are still active on Rightmove
 */
export const verifyCommand = defineCommand({
	meta: {
		name: 'verify',
		description: 'Check if listings are still active on Rightmove',
	},
	args: {
		'dry-run': { type: 'boolean', description: 'Preview without updating', default: false },
		region: { type: 'string', description: 'Only verify listings from specific region' },
		limit: { type: 'string', description: 'Max listings to check' },
		delay: { type: 'string', description: 'Delay between requests in ms', default: '3000' },
		json: { type: 'boolean', description: 'Output as JSON envelope', default: false },
	},
	async run({ args }) {
		const start = performance.now();
		const jsonMode = isJsonMode();
		const parsed = parseVerifyArgs(args);
		if (!parsed) return;

		setFetchDelay(parsed.delay);
		log.cli.info('Verify listings', { dryRun: parsed.dryRun, region: parsed.region ?? 'all', delay: parsed.delay });

		const existing = loadExistingListings();
		const emptyResult = { checked: 0, active: 0, inactive: 0, errors: 0, results: [] };

		if (existing.listings.length === 0) {
			if (jsonMode) ok('ops.verify', emptyResult, start);
			log.cli.warn('No listings to verify');
			return;
		}

		const toVerify = filterListingsForVerify(existing.listings, parsed.region, parsed.limit);
		if (toVerify.length === 0) {
			if (jsonMode) ok('ops.verify', emptyResult, start);
			log.cli.success('No listings to verify (all already have status)');
			return;
		}

		log.cli.info('Verifying listings', { count: toVerify.length, total: existing.listings.length });

		const { results, inactive, errors } = await runVerification(toVerify);
		const active = results.length - inactive - errors;

		if (!jsonMode) printVerifySummary(results.length, inactive, errors);

		if (parsed.dryRun) {
			if (jsonMode) ok('ops.verify', { checked: results.length, active, inactive, errors, dryRun: true, results }, start);
			log.cli.info('Dry run - no changes saved');
			return;
		}

		applyInactiveStatus(existing.listings, results);

		const output: ListingsFile = {
			updatedAt: new Date().toISOString(),
			searchUrls: existing.searchUrls,
			locations: existing.locations,
			lastSearchTotal: existing.lastSearchTotal,
			listings: existing.listings,
		};

		await saveListingsFile(output);

		if (jsonMode) ok('ops.verify', { checked: results.length, active, inactive, errors, results }, start);
		log.cli.success('Verification complete', { path: paths().derived.database, inactive });
	},
});
