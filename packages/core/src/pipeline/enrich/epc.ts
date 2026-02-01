/**
 * EPC enrichment module
 *
 * Fetches floor area and authoritative energy ratings from the EPC API,
 * matching records to listings by address.
 *
 * API: https://epc.opendatacommunities.org/api/v1/domestic/search
 * Authentication: Basic auth with email:api-key
 * Response format: CSV
 */

import type { Listing } from '@let/core/schema';
import { log } from '@let/core/utils/logger';
import { createResettableRateLimiter, sleep } from '../fetch/index.js';

// =============================================================================
// API CLIENT
// =============================================================================

/** Default delay between EPC API requests */
export const EPC_DELAY_MS = 1000;

/** Concurrency-safe rate limiter with jitter for EPC API */
const epcRateLimiter = createResettableRateLimiter(EPC_DELAY_MS);

/** Maximum retries for transient errors */
const MAX_RETRIES = 3;

/** Delay between retries (milliseconds) */
const RETRY_DELAY_MS = 2000;

/** Timeout for EPC API fetch requests (milliseconds) */
const EPC_FETCH_TIMEOUT_MS = 10000;

/** EPC API base URL */
const EPC_API_BASE = 'https://epc.opendatacommunities.org/api/v1/domestic/search';

/**
 * Parsed EPC record from API response
 */
export type EpcRecord = {
	address: string;
	postcode: string;
	epcRating: 'A' | 'B' | 'C' | 'D' | 'E' | 'F' | 'G';
	floorAreaSqm: number;
	propertyType: string;
	lodgementDate: string;
	uprn: string | null;
};

/**
 * Result of an EPC API fetch operation
 */
export type EpcApiResult = { success: true; records: EpcRecord[] } | { success: false; error: string };

/**
 * Build EPC API URL for postcode search
 */
function buildEpcUrl(postcode: string): string {
	const params = new URLSearchParams();
	params.set('postcode', postcode);
	return `${EPC_API_BASE}?${params.toString()}`;
}

/** Track whether we've warned about missing credentials */
let warnedMissingCredentials = false;

/**
 * Get EPC API credentials from environment
 */
function getCredentials(): { email: string; key: string } | null {
	const email = process.env['EPC_API_EMAIL'];
	const key = process.env['EPC_API_KEY'];

	if (!email || !key) {
		if (!warnedMissingCredentials) {
			log.enrich.warn('EPC API credentials not configured; EPC enrichment disabled');
			warnedMissingCredentials = true;
		}
		return null;
	}

	return { email, key };
}

/**
 * Build Basic Auth header from credentials
 */
function buildAuthHeader(email: string, key: string): string {
	const credentials = `${email}:${key}`;
	const encoded = Buffer.from(credentials).toString('base64');
	return `Basic ${encoded}`;
}

/** Calculate retry delay, respecting Retry-After header when present */
function getRetryDelay(response: Response, attempt: number): number {
	const retryAfter = response.headers.get('Retry-After');
	if (retryAfter) {
		const seconds = Number.parseInt(retryAfter, 10);
		if (!Number.isNaN(seconds)) {
			return seconds * 1000;
		}
	}
	return RETRY_DELAY_MS * attempt;
}

/** Column indices for CSV parsing */
type CsvColumnIndices = {
	address: number;
	postcode: number;
	rating: number;
	area: number;
	type: number;
	date: number;
	uprn: number;
};

/** Find column indices from CSV headers */
function findColumnIndices(headers: string[]): CsvColumnIndices | null {
	const indices: CsvColumnIndices = {
		address: headers.indexOf('address'),
		postcode: headers.indexOf('postcode'),
		rating: headers.indexOf('current-energy-rating'),
		area: headers.indexOf('total-floor-area'),
		type: headers.indexOf('property-type'),
		date: headers.indexOf('lodgement-date'),
		uprn: headers.indexOf('uprn'),
	};

	if (indices.address === -1 || indices.rating === -1 || indices.area === -1) {
		log.enrich.debug('Missing required CSV columns', { headers });
		return null;
	}
	return indices;
}

/** Parse a single CSV line, handling quoted fields and escaped quotes */
function parseCsvLine(line: string): string[] {
	const values: string[] = [];
	let current = '';
	let inQuotes = false;

	for (let i = 0; i < line.length; i++) {
		const char = line[i];

		if (char === '"') {
			if (inQuotes && line[i + 1] === '"') {
				current += '"';
				i++;
			} else {
				inQuotes = !inQuotes;
			}
		} else if (char === ',' && !inQuotes) {
			values.push(current.trim());
			current = '';
		} else {
			current += char;
		}
	}

	values.push(current.trim());
	return values;
}

/** Validate EPC rating is A-G */
function isValidRating(rating: string): boolean {
	return ['A', 'B', 'C', 'D', 'E', 'F', 'G'].includes(rating);
}

/** Parse a single CSV row into an EPC record */
function parseEpcRow(values: string[], indices: CsvColumnIndices): EpcRecord | null {
	const rating = values[indices.rating]?.toUpperCase();
	const area = Number.parseFloat(values[indices.area] ?? '');

	if (!rating || !isValidRating(rating) || Number.isNaN(area)) return null;

	return {
		address: values[indices.address] ?? '',
		postcode: indices.postcode !== -1 ? (values[indices.postcode] ?? '') : '',
		epcRating: rating as EpcRecord['epcRating'],
		floorAreaSqm: area,
		propertyType: indices.type !== -1 ? (values[indices.type] ?? '') : '',
		lodgementDate: indices.date !== -1 ? (values[indices.date] ?? '') : '',
		uprn: indices.uprn !== -1 ? (values[indices.uprn] ?? null) : null,
	};
}

/**
 * Parse CSV response from EPC API
 */
function parseCsv(csv: string): EpcRecord[] {
	const lines = csv.split('\n').filter((line) => line.trim());
	if (lines.length < 2 || !lines[0]) return [];

	const headers = parseCsvLine(lines[0]).map((h) => h.trim().toLowerCase());
	const indices = findColumnIndices(headers);
	if (!indices) return [];

	const records: EpcRecord[] = [];
	for (let i = 1; i < lines.length; i++) {
		const line = lines[i];
		if (!line) continue;

		const record = parseEpcRow(parseCsvLine(line), indices);
		if (record) records.push(record);
	}
	return records;
}

/** Handle EPC API response */
async function handleEpcResponse(response: Response, postcode: string, attempt: number): Promise<EpcApiResult | 'retry' | null> {
	if (response.ok) {
		const csv = await response.text();
		const records = parseCsv(csv);
		log.enrich.debug('EPC API success', { postcode, recordCount: records.length });
		return { success: true, records };
	}

	if (response.status === 404) {
		log.enrich.debug('No EPC data found', { postcode });
		return { success: true, records: [] };
	}

	if (response.status === 429 && attempt < MAX_RETRIES) {
		const backoff = getRetryDelay(response, attempt);
		log.enrich.debug('EPC API rate limited, backing off', { backoff });
		await sleep(backoff);
		return 'retry';
	}

	if (response.status === 401 || response.status === 403) {
		return { success: false, error: `EPC API authentication failed (${response.status})` };
	}

	if (response.status >= 400 && response.status < 500) {
		return { success: false, error: `EPC API error: ${response.status}` };
	}

	return null;
}

/** EPC attempt result */
type EpcAttemptResult = { done: true; result: EpcApiResult } | { done: false; error: string; retryDelayMs?: number };

/** Execute a single EPC fetch attempt with timeout */
async function executeEpcAttempt(url: string, credentials: { email: string; key: string }, postcode: string, attempt: number): Promise<EpcAttemptResult> {
	const controller = new AbortController();
	const timeout = setTimeout(() => controller.abort(), EPC_FETCH_TIMEOUT_MS);

	try {
		const response = await fetch(url, {
			signal: controller.signal,
			headers: { Authorization: buildAuthHeader(credentials.email, credentials.key), Accept: 'text/csv' },
		});

		const result = await handleEpcResponse(response, postcode, attempt);
		if (result === 'retry') return { done: false, error: 'Rate limited, retrying', retryDelayMs: 0 };
		if (result) return { done: true, result };

		return { done: false, error: `EPC API server error: ${response.status}` };
	} finally {
		clearTimeout(timeout);
	}
}

/**
 * Fetch EPC data for a postcode
 *
 * @param postcode - UK postcode to search
 * @returns Array of EPC records or error
 */
export async function fetchEpcByPostcode(postcode: string): Promise<EpcApiResult> {
	const credentials = getCredentials();
	if (!credentials) return { success: false, error: 'EPC API credentials not configured' };

	await epcRateLimiter.throttle();
	const url = buildEpcUrl(postcode);
	let lastError = 'Unknown error';

	for (let attempt = 1; attempt <= MAX_RETRIES; attempt++) {
		let retryDelayMs = RETRY_DELAY_MS;
		try {
			const result = await executeEpcAttempt(url, credentials, postcode, attempt);
			if (result.done) return result.result;
			lastError = result.error;
			retryDelayMs = result.retryDelayMs ?? RETRY_DELAY_MS;
		} catch (e) {
			lastError = e instanceof Error ? e.message : 'Network error';
			retryDelayMs = RETRY_DELAY_MS;
		}
		if (attempt < MAX_RETRIES && retryDelayMs > 0) {
			await sleep(retryDelayMs);
		}
	}

	return { success: false, error: lastError };
}

/**
 * Reset EPC rate limiter (useful for testing)
 */
export function resetEpcRateLimiter(): void {
	epcRateLimiter.reset();
}

// =============================================================================
// ADDRESS MATCHING
// =============================================================================

/**
 * Normalized address components for matching
 */
export type NormalizedAddress = {
	number: string; // "123" (digits only)
	numberSuffix: string; // "a" (lowercase letter suffix)
	flat: string; // "2" (flat/unit number)
	streetName: string; // "high" (first word, lowercase)
	streetType: string; // "road" (expanded to full form)
	original: string;
};

/** Map abbreviations to full street type names */
const STREET_TYPE_EXPANSIONS: Record<string, string> = {
	rd: 'road',
	st: 'street',
	ln: 'lane',
	ave: 'avenue',
	av: 'avenue',
	dr: 'drive',
	cl: 'close',
	ct: 'court',
	cres: 'crescent',
	gdns: 'gardens',
	gdn: 'garden',
	ter: 'terrace',
	pl: 'place',
	sq: 'square',
	way: 'way',
	gr: 'grove',
	pk: 'park',
	hl: 'hill',
	vw: 'view',
	mews: 'mews',
	row: 'row',
	walk: 'walk',
	rise: 'rise',
	gate: 'gate',
};

/** Known full street type names */
const STREET_TYPES = new Set([
	'road',
	'street',
	'lane',
	'avenue',
	'drive',
	'close',
	'court',
	'crescent',
	'gardens',
	'garden',
	'terrace',
	'place',
	'square',
	'way',
	'grove',
	'park',
	'hill',
	'view',
	'mews',
	'row',
	'walk',
	'rise',
	'gate',
]);

/**
 * Extract flat/unit number from address
 * Handles: "Flat 2", "Unit 2", "Apartment 2", "Apt 2"
 */
function extractFlat(address: string): { flat: string; remaining: string } {
	const flatMatch = address.match(/^(?:flat|unit|apartment|apt)\s*(\d+[a-z]?)\s*[,.]?\s*/i);
	if (flatMatch?.[1]) {
		return { flat: flatMatch[1].toLowerCase(), remaining: address.slice(flatMatch[0].length) };
	}
	return { flat: '', remaining: address };
}

/**
 * Extract house number and suffix from address start
 * Handles: "123", "123A", "123a", "12/A", "12-A", "12/a"
 */
function extractNumber(address: string): { number: string; suffix: string; remaining: string } {
	// Match: digits optionally followed by separator + letter OR just letter
	const match = address.match(/^(\d+)(?:[-/]?([a-z]))?(?:\s+|,|$)/i);
	if (match?.[1]) {
		return {
			number: match[1],
			suffix: (match[2] ?? '').toLowerCase(),
			remaining: address.slice(match[0].length).trim(),
		};
	}
	return { number: '', suffix: '', remaining: address };
}

/**
 * Extract street name and type from remaining address
 * Expands abbreviations to full form (rd -> road)
 */
function extractStreet(address: string): { streetName: string; streetType: string } {
	const words = address
		.toLowerCase()
		.replace(/[.,;:'"]/g, '')
		.split(/\s+/)
		.filter(Boolean);

	if (words.length === 0) return { streetName: '', streetType: '' };

	// Find street type (last word that matches a known type or abbreviation)
	let streetType = '';

	for (let i = words.length - 1; i >= 0; i--) {
		const word = words[i];
		if (!word) continue;

		if (STREET_TYPES.has(word)) {
			streetType = word;
			break;
		}
		if (STREET_TYPE_EXPANSIONS[word]) {
			streetType = STREET_TYPE_EXPANSIONS[word];
			break;
		}
	}

	// Street name is the first word before the type
	// If no type found, use first word as street name
	const streetName = words[0] ?? '';

	return { streetName, streetType };
}

/**
 * Normalize an address string for comparison
 * Extracts structured components for reliable matching
 */
export function normalizeAddress(address: string): NormalizedAddress {
	const original = address;

	// Step 1: Extract flat number if present
	const { flat, remaining: afterFlat } = extractFlat(address.trim());

	// Step 2: Extract house number and suffix
	const { number, suffix, remaining: afterNumber } = extractNumber(afterFlat);

	// Step 3: Extract street name and type
	const { streetName, streetType } = extractStreet(afterNumber);

	return {
		number,
		numberSuffix: suffix,
		flat,
		streetName,
		streetType,
		original,
	};
}

/**
 * Calculate Levenshtein distance between two strings
 * Uses optimized single-row algorithm with O(min(m,n)) space
 */
export function levenshteinDistance(a: string, b: string): number {
	if (a === b) return 0;
	if (a.length === 0) return b.length;
	if (b.length === 0) return a.length;

	// Ensure a is the shorter string for space optimization
	if (a.length > b.length) [a, b] = [b, a];

	const row = Array.from({ length: a.length + 1 }, (_, i) => i);

	for (let i = 1; i <= b.length; i++) {
		let prev = i;
		for (let j = 1; j <= a.length; j++) {
			const current = a[j - 1] === b[i - 1] ? (row[j - 1] ?? 0) : Math.min(row[j - 1] ?? 0, Math.min(prev, row[j] ?? 0)) + 1;
			row[j - 1] = prev;
			prev = current;
		}
		row[a.length] = prev;
	}
	return row[a.length] ?? 0;
}

/**
 * Check if two addresses match with high confidence
 * House number and suffix must match exactly, street type must match,
 * street name uses Levenshtein distance <= 1 as fuzzy fallback
 */
export function addressesMatch(a: NormalizedAddress, b: NormalizedAddress): boolean {
	// House number must be present and match exactly
	if (!a.number || !b.number) return false;
	if (a.number !== b.number) return false;

	// Number suffix must match exactly
	if (a.numberSuffix !== b.numberSuffix) return false;

	// Flat must match if present in either address
	if ((a.flat || b.flat) && a.flat !== b.flat) return false;

	// Street type must match exactly (road vs street distinction matters)
	if (a.streetType !== b.streetType) return false;

	// Street name: exact match preferred
	if (a.streetName === b.streetName) return true;

	// Fuzzy fallback: allow Levenshtein distance <= 1 for minor typos
	if (a.streetName && b.streetName) {
		return levenshteinDistance(a.streetName, b.streetName) <= 1;
	}

	return false;
}

/** Match classification result */
type MatchResult = { exact: EpcRecord[]; sameStreet: EpcRecord[] };

/** Classify EPC records into exact and same-street matches */
function classifyMatches(normalizedListing: NormalizedAddress, epcRecords: EpcRecord[]): MatchResult {
	const exact: EpcRecord[] = [];
	const sameStreet: EpcRecord[] = [];

	for (const record of epcRecords) {
		const normalizedEpc = normalizeAddress(record.address);

		if (addressesMatch(normalizedListing, normalizedEpc)) {
			exact.push(record);
		} else if (
			normalizedListing.streetName &&
			normalizedListing.streetType &&
			normalizedEpc.streetName === normalizedListing.streetName &&
			normalizedEpc.streetType === normalizedListing.streetType
		) {
			// Same street (exact match on both street name and type)
			sameStreet.push(record);
		}
	}

	return { exact, sameStreet };
}

/** Get median record from array sorted by floor area */
function getMedianRecord(records: EpcRecord[]): EpcRecord | null {
	if (records.length === 0) return null;
	const sorted = [...records].sort((a, b) => a.floorAreaSqm - b.floorAreaSqm);
	return sorted[Math.floor(sorted.length / 2)] ?? null;
}

/**
 * Find the best matching EPC record for a listing address
 *
 * Matching strategy:
 * 1. Exact address match (single result) -> return that record
 * 2. Multiple exact matches (flats at same address) -> ambiguous, return null
 * 3. No house number in listing, same street records exist -> return median by floor area
 * 4. No match -> return null
 */
function findBestMatch(listingAddress: string, epcRecords: EpcRecord[]): { record: EpcRecord; source: 'exact' | 'street-median' } | null {
	if (epcRecords.length === 0) return null;

	const normalizedListing = normalizeAddress(listingAddress);
	const { exact, sameStreet } = classifyMatches(normalizedListing, epcRecords);

	// Ambiguous match: multiple exact matches found (different flats/units at same address)
	if (exact.length > 1) {
		log.enrich.warn('Ambiguous EPC match (multiple exact matches)', {
			listingAddress,
			matchCount: exact.length,
			addresses: exact.map((r) => r.address),
		});
		return null;
	}

	// Single exact match
	if (exact.length === 1 && exact[0]) {
		return { record: exact[0], source: 'exact' };
	}

	// No exact match - try street median fallback if listing has no house number
	if (!normalizedListing.number && sameStreet.length > 0) {
		const median = getMedianRecord(sameStreet);
		if (median) {
			log.enrich.debug('Using street-median EPC fallback', {
				listingAddress,
				streetRecordCount: sameStreet.length,
				medianFloorArea: median.floorAreaSqm,
				medianRating: median.epcRating,
			});
			return { record: median, source: 'street-median' };
		}
	}

	return null;
}

// =============================================================================
// ENRICHMENT API
// =============================================================================

/**
 * Result of EPC enrichment
 * success: true with epc: null means no EPC records found (not an error)
 * success: false with error means API/network failure
 * matchSource: 'exact' for direct address match, 'street-median' for fallback
 */
export type EpcEnrichmentResult = { success: true; epc: EpcRecord | null; matched: boolean; matchSource?: 'exact' | 'street-median' } | { success: false; error: string };

/**
 * Enrich a listing with EPC data
 *
 * Fetches EPC records for the listing's postcode and attempts to match
 * the correct record by address.
 *
 * @param listing - The listing to enrich
 * @returns Enrichment result with matched EPC data or null
 */
export async function enrichWithEpc(listing: Listing): Promise<EpcEnrichmentResult> {
	if (!listing.postcode) {
		return { success: true, epc: null, matched: false };
	}

	const result = await fetchEpcByPostcode(listing.postcode);

	if (!result.success) {
		// Log warning and propagate error - distinguishes API failures from "no records"
		log.enrich.warn('EPC API error', { postcode: listing.postcode, error: result.error });
		return { success: false, error: result.error };
	}

	if (result.records.length === 0) {
		return { success: true, epc: null, matched: false };
	}

	const match = findBestMatch(listing.address, result.records);

	if (!match) {
		log.enrich.debug('No address match found', {
			listingAddress: listing.address,
			postcode: listing.postcode,
			epcRecordCount: result.records.length,
		});
		return { success: true, epc: null, matched: false };
	}

	log.enrich.debug('EPC match found', {
		listingAddress: listing.address,
		epcAddress: match.record.address,
		rating: match.record.epcRating,
		area: match.record.floorAreaSqm,
		source: match.source,
	});

	return { success: true, epc: match.record, matched: true, matchSource: match.source };
}

/**
 * Apply EPC enrichment data to a listing
 *
 * Mutates the listing to add EPC fields if a match was found.
 * Returns whether enrichment was applied.
 */
export function applyEpcToListing(listing: Listing, result: EpcEnrichmentResult): boolean {
	if (!result.success || !result.epc) {
		return false;
	}

	listing.epcRating = result.epc.epcRating;
	listing.floorAreaSqm = result.epc.floorAreaSqm;
	listing.epcLodgementDate = result.epc.lodgementDate;
	listing.epcAddressMatch = result.matched;
	if (result.epc.uprn) {
		listing.uprn = result.epc.uprn;
		listing.uprnSource = 'epc';
		listing.uprnConfidence = 'exact';
	}

	return true;
}
