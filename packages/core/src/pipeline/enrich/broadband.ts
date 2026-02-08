/**
 * Broadband coverage lookup utility
 *
 * Queries local SQLite database of UK broadband coverage (Ofcom data).
 * Returns gigabit availability percentage for scoring.
 *
 * Fallback chain: exact postcode -> district aggregate -> area aggregate
 */

import { Database } from 'bun:sqlite';
import { log } from '@let/core/utils/logger';
import { paths } from '../../paths.js';

/**
 * Result of a broadband lookup
 */
export interface BroadbandResult {
	/** Gigabit (1Gbps) availability percentage (0-100) */
	gigabitAvailability: number;
	/** Data source: exact postcode, district aggregate, or area aggregate */
	source: 'postcode' | 'outward' | 'area';
}

/**
 * Resolve database path using shared path resolution
 */
function resolveDatabasePath(): string {
	return paths().derived.sourceDb('broadband');
}

/** Cached database connection (singleton) */
let db: Database | null = null;

/** Track if database initialization failed (avoid repeated attempts) */
let dbInitFailed = false;

/**
 * Get database connection (lazy initialization)
 * Returns null if database is unavailable (missing or corrupt)
 */
function getDb(): Database | null {
	if (db) return db;
	if (dbInitFailed) return null;

	try {
		db = new Database(resolveDatabasePath(), { readonly: true });
		return db;
	} catch (e) {
		dbInitFailed = true;
		log.enrich.warn('Broadband database unavailable', { error: String(e) });
		return null;
	}
}

/**
 * Normalize postcode for lookup (uppercase, no spaces)
 */
function normalizePostcode(postcode: string): string {
	return postcode.replace(/\s+/g, '').toUpperCase();
}

/**
 * Check if query is a full UK postcode (5-7 chars without space)
 */
function isFullPostcode(query: string): boolean {
	return query.length >= 5;
}

/**
 * Check if query is an outward code (2-4 chars with letter+number)
 */
function isOutwardCode(query: string): boolean {
	return query.length >= 2 && query.length <= 4 && /[A-Z]/.test(query) && /\d/.test(query);
}

/**
 * Extract outward code from full postcode
 * UK postcodes: outward (2-4 chars) + inward (3 chars)
 */
function extractOutward(postcode: string): string {
	return postcode.slice(0, -3);
}

/**
 * Extract area code from postcode (1-2 letters at start)
 * Uses regex to match leading letters only, handling single-letter areas like M, B, G
 */
function extractArea(postcode: string): string {
	const match = postcode.match(/^[A-Z]+/);
	return match ? match[0] : '';
}

/**
 * Lookup broadband coverage for a postcode
 *
 * Tries exact postcode match first, then falls back to district (outward)
 * and area aggregates if no exact match found.
 *
 * @param postcode - UK postcode (with or without space)
 * @returns BroadbandResult with gigabit availability, or null if not found
 * @note Database stores gigabit_availability as 0-100 percentage, not 0-1 fraction
 */
export function lookupBroadband(postcode: string): BroadbandResult | null {
	const query = normalizePostcode(postcode);
	const database = getDb();
	if (!database) return null;

	try {
		// Full postcode: try exact match first
		if (isFullPostcode(query)) {
			const row = database.query('SELECT gigabit_availability FROM postcodes WHERE postcode = ?').get(query) as { gigabit_availability: number } | null;

			if (row) {
				return {
					gigabitAvailability: row.gigabit_availability,
					source: 'postcode',
				};
			}
		}

		// Outward code fallback (works for both full and partial postcodes)
		const outward = isFullPostcode(query) ? extractOutward(query) : query;
		if (isOutwardCode(outward)) {
			const outwardRow = database.query('SELECT avg_gigabit_availability FROM outward_aggregates WHERE outward = ?').get(outward) as { avg_gigabit_availability: number } | null;

			if (outwardRow) {
				return {
					gigabitAvailability: Math.round(outwardRow.avg_gigabit_availability * 10) / 10,
					source: 'outward',
				};
			}
		}

		// Area fallback
		const area = extractArea(query);
		if (area.length >= 1) {
			const areaRow = database.query('SELECT avg_gigabit_availability FROM area_aggregates WHERE area = ?').get(area) as { avg_gigabit_availability: number } | null;

			if (areaRow) {
				return {
					gigabitAvailability: Math.round(areaRow.avg_gigabit_availability * 10) / 10,
					source: 'area',
				};
			}
		}

		return null;
	} catch (e) {
		log.enrich.warn('Broadband lookup failed', { postcode, error: String(e) });
		return null;
	}
}

/**
 * Close database connection (for cleanup/testing)
 */
export function closeBroadbandDb(): void {
	if (db) {
		db.close();
		db = null;
	}
}
