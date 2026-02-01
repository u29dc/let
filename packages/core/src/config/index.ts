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
// Re-export types
export type { Config, FetchConfig, Location, ScoringConfig, SearchConfig, SearchFilters } from './types.js';
