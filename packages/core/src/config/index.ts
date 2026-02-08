/**
 * Configuration Module
 *
 * Provides configuration loading and validation for the property search system.
 */

// Re-export loader functions and schemas
export {
	ConfigSchema,
	DEFAULT_SCORING_CONFIG,
	loadConfig,
	loadScoringConfig,
	parseScoringConfig,
	resetConfigCache,
	ScoringConfigSchema,
} from './loader.js';
export type { Config, FetchConfig, Location, RightmoveSearchType, ScoringConfig, SearchConfig, SearchFilters } from './types.js';
// Re-export constants
export { RIGHTMOVE_SEARCH_TYPES } from './types.js';
