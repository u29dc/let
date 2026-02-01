import { z } from 'zod';

import { ListingSchema } from './listing.js';

// =============================================================================
// SEARCH RESULTS SCHEMA
// =============================================================================

export const SearchResultsSchema = z.object({
	searchUrl: z.string(),
	region: z.string(),
	fetchedAt: z.string().datetime(),
	totalResults: z.number(),
	listings: z.array(ListingSchema),
});

export type SearchResults = z.infer<typeof SearchResultsSchema>;

// =============================================================================
// LISTINGS FILE SCHEMA (for JSON export)
// =============================================================================

export const ListingsFileSchema = z.object({
	/** ISO datetime when listings export was last updated */
	updatedAt: z.string().datetime(),
	/** All unique search URLs used across runs */
	searchUrls: z.array(z.string()),
	/** All unique locations searched across runs */
	locations: z.array(z.string()),
	/** Total results from last search */
	lastSearchTotal: z.number(),
	/** All listings, deduplicated by ID */
	listings: z.array(ListingSchema),
});

export type ListingsFile = z.infer<typeof ListingsFileSchema>;
