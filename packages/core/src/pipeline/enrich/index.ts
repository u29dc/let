/**
 * Pipeline Stage 3: Enrich
 *
 * Enrichment of listings with external data sources:
 * - EPC API (floor area, energy rating)
 * - Broadband (Ofcom gigabit availability)
 * - Area metrics (IMD, census, flood, income, crime)
 * - Notes (pattern-based extraction from descriptions)
 */

import type { Listing } from '@let/core/schema';
import { log } from '@let/core/utils/logger';

export { type AreaEnrichmentResult, closeAreaDbs, enrichListingArea, lookupPostcode } from './area.js';
// Re-export broadband lookup
export { type BroadbandResult, closeBroadbandDb, lookupBroadband } from './broadband.js';
// Re-export EPC enrichment
export {
	applyEpcToListing,
	EPC_DELAY_MS,
	type EpcApiResult,
	type EpcEnrichmentResult,
	type EpcRecord,
	enrichWithEpc,
	fetchEpcByPostcode,
	resetEpcRateLimiter,
} from './epc.js';

// Re-export notes extraction
export { type EnrichNotesResult, enrichListingNotes, extractNotes } from './notes/index.js';

import { enrichListingArea } from './area.js';
import { lookupBroadband } from './broadband.js';
// Import for orchestration
import { applyEpcToListing, enrichWithEpc } from './epc.js';
import { enrichListingNotes } from './notes/index.js';

/**
 * Options for the enrichment stage
 */
export interface EnrichOptions {
	/** Skip EPC API enrichment */
	skipEpc?: boolean;
	/** Skip broadband lookup */
	skipBroadband?: boolean;
	/** Skip area metrics lookup */
	skipArea?: boolean;
	/** Skip notes extraction */
	skipNotes?: boolean;
	/** Dev mode (skip remote API calls) */
	dev?: boolean;
}

/**
 * Result of enrichment for a listing
 */
export interface EnrichResult {
	/** Whether EPC enrichment was applied */
	epcApplied: boolean;
	/** Whether broadband lookup was applied */
	broadbandApplied: boolean;
	/** Whether area enrichment was applied */
	areaApplied: boolean;
	/** Whether notes were extracted */
	notesExtracted: boolean;
}

/** Apply EPC enrichment (skip in dev mode or if explicitly disabled) */
async function applyEpcEnrichment(listing: Listing, options: EnrichOptions): Promise<boolean> {
	if (options.skipEpc || options.dev || !listing.postcode) return false;

	const epcResult = await enrichWithEpc(listing);
	if (!applyEpcToListing(listing, epcResult)) return false;

	log.enrich.success('EPC enriched', {
		id: listing.id,
		rating: listing.epcRating,
		area: listing.floorAreaSqm,
	});
	return true;
}

/** Apply broadband enrichment (local SQLite lookup, no API call) */
function applyBroadbandEnrichment(listing: Listing, options: EnrichOptions): boolean {
	if (options.skipBroadband || !listing.postcode) return false;

	const broadbandResult = lookupBroadband(listing.postcode);
	if (!broadbandResult) return false;

	listing.gigabitAvailability = broadbandResult.gigabitAvailability;
	log.enrich.debug('Broadband enriched', {
		id: listing.id,
		postcode: listing.postcode,
		gigabit: broadbandResult.gigabitAvailability,
		source: broadbandResult.source,
	});
	return true;
}

/** Apply area metrics enrichment (local SQLite lookup, no API call) */
function applyAreaEnrichment(listing: Listing, options: EnrichOptions): boolean {
	if (options.skipArea || !listing.postcode) return false;

	const areaResult = enrichListingArea(listing);
	if (!areaResult.applied) return false;

	log.enrich.debug('Area metrics enriched', {
		id: listing.id,
		postcode: listing.postcode,
		lsoa: listing.area.lsoa.code,
	});
	return true;
}

/** Apply notes extraction (pattern-based, no API call) */
function applyNotesEnrichment(listing: Listing, options: EnrichOptions): boolean {
	if (options.skipNotes || !listing.description) return false;

	const notesResult = enrichListingNotes(listing);
	if (!notesResult.success || notesResult.notes.length === 0) return false;

	listing.notes = notesResult.notes;
	return true;
}

/**
 * Enrich a listing by applying all enrichments
 *
 * This is the main orchestration function for Stage 3.
 * Applies EPC, broadband, and notes enrichment to a listing.
 *
 * @param listing - The listing to enrich (mutated in place)
 * @param options - Options to skip specific enrichments
 * @returns Summary of which enrichments were applied
 */
export async function enrichListing(listing: Listing, options: EnrichOptions = {}): Promise<EnrichResult> {
	return {
		epcApplied: await applyEpcEnrichment(listing, options),
		broadbandApplied: applyBroadbandEnrichment(listing, options),
		areaApplied: applyAreaEnrichment(listing, options),
		notesExtracted: applyNotesEnrichment(listing, options),
	};
}

/**
 * Enrich multiple listings with progress logging
 *
 * @param listings - Array of listings to enrich
 * @param options - Options to skip specific enrichments
 * @returns Array of enrichment results
 */
export async function enrichListings(listings: Listing[], options: EnrichOptions = {}): Promise<EnrichResult[]> {
	const results: EnrichResult[] = [];

	for (let i = 0; i < listings.length; i++) {
		const listing = listings[i];
		if (!listing) continue;

		log.enrich.debug('Enriching listing', { current: i + 1, total: listings.length, id: listing.id });
		const result = await enrichListing(listing, options);
		results.push(result);
	}

	const summary = {
		total: results.length,
		epcEnriched: results.filter((r) => r.epcApplied).length,
		broadbandEnriched: results.filter((r) => r.broadbandApplied).length,
		areaEnriched: results.filter((r) => r.areaApplied).length,
		notesExtracted: results.filter((r) => r.notesExtracted).length,
	};

	log.enrich.success('Enrichment complete', summary);

	return results;
}
