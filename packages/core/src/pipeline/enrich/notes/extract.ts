/**
 * Note extraction enrichment module
 *
 * Extracts clean, useful observations from listing descriptions using
 * pattern-based text matching. Populates the `notes` array with concise
 * property features.
 *
 * Categories extracted:
 * - Condition signals (refurbished, new kitchen, dated decor)
 * - Garden details (south-facing, enclosed, with measurements)
 * - Parking (driveway, garage, allocated)
 * - Useful features (bay windows, en-suite, utility room)
 * - Location benefits (near station, quiet cul-de-sac)
 * - Red flags (above commercial, shared entrance)
 * - Heating/utilities (gas central heating, double glazing)
 * - Tenancy terms (pets considered, minimum term)
 */

import type { Listing } from '@let/core/schema';
import { log } from '@let/core/utils/logger';
import { CATEGORY_ORDER, NOTE_PATTERNS } from './patterns.js';

/**
 * Result of note extraction enrichment
 */
export type EnrichNotesResult = { success: true; notes: string[]; changed: boolean } | { success: false; error: string };

/**
 * Filter out notes that are redundant with structured listing data
 */
function filterRedundantNotes(notes: Set<string>, listing: Listing): string[] {
	return [...notes].filter((note) => {
		if (note.includes(`${listing.bedrooms} bed`)) return false;
		if (note === `${listing.bedrooms} bedrooms`) return false;
		if (listing.epcRating && note.toLowerCase().includes(`epc ${listing.epcRating.toLowerCase()}`)) return false;
		if (/\d+\s*sq\s*(?:ft|m)/i.test(note)) return false;
		return true;
	});
}

/**
 * Sort notes by category importance
 */
function sortNotesByCategory(notes: string[]): string[] {
	return notes.sort((a, b) => {
		const aIndex = CATEGORY_ORDER.findIndex((cat) => a.toLowerCase().startsWith(cat));
		const bIndex = CATEGORY_ORDER.findIndex((cat) => b.toLowerCase().startsWith(cat));
		const aScore = aIndex === -1 ? 100 : aIndex;
		const bScore = bIndex === -1 ? 100 : bIndex;
		return aScore - bScore;
	});
}

/**
 * Extract notes from a listing description
 *
 * Pure function that applies pattern matching to extract useful observations
 * from the description text, filters redundant info, and sorts by category.
 *
 * @param listing - The listing to extract notes from
 * @returns Array of extracted notes (lowercase, concise)
 */
export function extractNotes(listing: Listing): string[] {
	if (!listing.description) {
		return [];
	}

	const description = listing.description.toLowerCase();
	const notes = new Set<string>();

	for (const { pattern, extract } of NOTE_PATTERNS) {
		const matches = description.matchAll(new RegExp(pattern));
		for (const match of matches) {
			const note = extract(match);
			if (note) {
				notes.add(note);
			}
		}
	}

	const filtered = filterRedundantNotes(notes, listing);
	return sortNotesByCategory(filtered);
}

/**
 * Enrich a listing with extracted notes
 *
 * Wrapper around extractNotes that handles logging and returns a result type.
 * Does not mutate the listing - caller should apply the notes.
 *
 * @param listing - The listing to enrich
 * @returns Enrichment result with notes array and changed flag
 */
export function enrichListingNotes(listing: Listing): EnrichNotesResult {
	try {
		const notes = extractNotes(listing);
		const changed = notes.length !== listing.notes.length || notes.some((n, i) => n !== listing.notes[i]);

		if (notes.length > 0) {
			log.enrich.debug('Notes extracted', {
				id: listing.id,
				count: notes.length,
				sample: notes.slice(0, 3).join(', '),
			});
		}

		return { success: true, notes, changed };
	} catch (error) {
		const message = error instanceof Error ? error.message : String(error);
		log.enrich.error('Failed to extract notes', { id: listing.id, error: message });
		return { success: false, error: message };
	}
}
