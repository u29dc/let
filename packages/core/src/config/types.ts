/**
 * Configuration Type Definitions
 *
 * All configuration types for the property search system.
 * Scoring-specific types remain in pipeline/score/types.ts.
 */

import type { ScoringConfig } from '../pipeline/score/types.js';

// =============================================================================
// RIGHTMOVE SEARCH TYPES
// =============================================================================

/** Valid property types for Rightmove search URL filters (not listing subtypes) */
export const RIGHTMOVE_SEARCH_TYPES = ['detached', 'semi-detached', 'terraced', 'flat'] as const;
export type RightmoveSearchType = (typeof RIGHTMOVE_SEARCH_TYPES)[number];

// =============================================================================
// SEARCH CONFIGURATION
// =============================================================================

/**
 * Location identifier for Rightmove search
 */
export interface Location {
	id: string;
	name: string;
}

/**
 * Search filter parameters
 */
export interface SearchFilters {
	minBedrooms: number;
	maxBedrooms: number;
	minPrice: number;
	maxPrice: number;
	propertyTypes: string[];
	includeLetAgreed: boolean;
	radius: number;
	dontShow: string[];
	mustHave: string[];
}

/**
 * Search configuration section
 */
export interface SearchConfig {
	locations: Location[];
	filters: SearchFilters;
}

// =============================================================================
// FETCH CONFIGURATION
// =============================================================================

/**
 * Fetch behavior configuration
 */
export interface FetchConfig {
	delayMs: number;
	maxListings: number;
	maxRetries: number;
}

// =============================================================================
// FULL CONFIGURATION
// =============================================================================

/**
 * Complete application configuration
 */
export interface Config {
	search: SearchConfig;
	fetch: FetchConfig;
	scoring: ScoringConfig;
}

// Re-export scoring types for convenience
export type { ScoringConfig } from '../pipeline/score/types.js';
