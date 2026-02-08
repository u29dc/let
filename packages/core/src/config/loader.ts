/**
 * Configuration Loading and Validation
 *
 * Loads config.toml using Bun's TOML parsing with Zod validation.
 * Merges search config with scoring config from two sources.
 */

import { z } from 'zod/v4';
import { log } from '../utils/logger.js';
import type { Config, ScoringConfig } from './types.js';

// =============================================================================
// SEARCH & FETCH SCHEMAS
// =============================================================================

const LocationSchema = z.object({
	id: z.string(),
	name: z.string(),
});

const FiltersSchema = z.object({
	minBedrooms: z.number().int().nonnegative(),
	maxBedrooms: z.number().int().positive(),
	minPrice: z.number().nonnegative(),
	maxPrice: z.number().positive(),
	propertyTypes: z.array(z.string()),
	includeLetAgreed: z.boolean(),
	radius: z.number().nonnegative(),
	dontShow: z.array(z.string()),
	mustHave: z.array(z.string()),
});

const FetchSchema = z.object({
	useApi: z.boolean().default(false),
	delayMs: z.number().int().positive(),
	maxListings: z.number().int().positive(),
	maxRetries: z.number().int().positive(),
});

// =============================================================================
// SCORING SCHEMAS
// =============================================================================

const CompositeWeightsSchema = z
	.object({
		affordability: z.number().min(0).max(1),
		location: z.number().min(0).max(1),
		liveability: z.number().min(0).max(1),
	})
	.refine((w) => Math.abs(w.affordability + w.location + w.liveability - 1) < 0.01, {
		message: 'Composite weights must sum to 1.0',
	});

const HeatingCostsSchema = z.object({
	A: z.number().nonnegative(),
	B: z.number().nonnegative(),
	C: z.number().nonnegative(),
	D: z.number().nonnegative(),
	E: z.number().nonnegative(),
	F: z.number().nonnegative(),
	G: z.number().nonnegative(),
});

const AffordabilityConfigSchema = z
	.object({
		priceWeight: z.number().min(0).max(1),
		epcWeight: z.number().min(0).max(1),
		heatingCosts: HeatingCostsSchema,
	})
	.refine((c) => Math.abs(c.priceWeight + c.epcWeight - 1.0) < 0.01, {
		message: 'Affordability weights must sum to 1.0 (priceWeight + epcWeight)',
	});

const LocationConfigSchema = z
	.object({
		stationWeight: z.number().min(0).max(1),
		broadbandWeight: z.number().min(0).max(1),
		priorityWeight: z.number().min(0).max(1),
		imdWeight: z.number().min(0).max(1),
		crimeWeight: z.number().min(0).max(1),
	})
	.refine((c) => Math.abs(c.stationWeight + c.broadbandWeight + c.priorityWeight + c.imdWeight + c.crimeWeight - 1.0) < 0.01, {
		message: 'Location weights must sum to 1.0',
	});

const GardenScoresSchema = z.object({
	private: z.number().min(0).max(100),
	shared: z.number().min(0).max(100),
	none: z.number().min(0).max(100),
});

const HeatingScoresSchema = z.object({
	gas: z.number().min(0).max(100),
	electric: z.number().min(0).max(100),
	unknown: z.number().min(0).max(100),
});

const LiveabilityConfigSchema = z
	.object({
		gardenWeight: z.number().min(0).max(1),
		heatingWeight: z.number().min(0).max(1),
		propertyTypeWeight: z.number().min(0).max(1),
		garden: GardenScoresSchema,
		heating: HeatingScoresSchema,
		propertyType: z.record(z.string(), z.number().min(0).max(100)),
	})
	.refine((c) => Math.abs(c.gardenWeight + c.heatingWeight + c.propertyTypeWeight - 1.0) < 0.01, {
		message: 'Liveability weights must sum to 1.0',
	});

const PenaltyConfigSchema = z.object({
	epcF: z.number().min(0).max(1),
	epcG: z.number().min(0).max(1),
	noGarden: z.number().min(0).max(1),
	noPets: z.number().min(0).max(1),
	missingDataPenalty: z.number().min(0).max(1).default(0.95),
	gardenRequired: z.boolean().default(false), // Injected from mustHave, default false
});

export const ScoringConfigSchema = z.object({
	adaptiveness: z.number().min(0.5).max(10).default(2.0),
	adaptivenessFactor: z.number().min(0.1).max(20).default(10),
	weights: CompositeWeightsSchema,
	affordability: AffordabilityConfigSchema,
	location: LocationConfigSchema,
	liveability: LiveabilityConfigSchema,
	penalties: PenaltyConfigSchema,
	regionPriority: z.record(z.string(), z.number().min(0).max(100)),
});

// =============================================================================
// FULL CONFIG SCHEMA
// =============================================================================

export const ConfigSchema = z.object({
	search: z.object({
		locations: z.array(LocationSchema).min(1),
		filters: FiltersSchema,
	}),
	fetch: FetchSchema,
	scoring: ScoringConfigSchema,
});

// =============================================================================
// DEFAULT CONFIGURATION
// =============================================================================

/**
 * Default scoring configuration
 */
export const DEFAULT_SCORING_CONFIG: ScoringConfig = {
	adaptiveness: 2.0, // 1.0=conservative, 2.0=balanced, 4.0=aggressive compensation
	adaptivenessFactor: 10, // Sigmoid steepness multiplier (exposes previous hardcoded factor)
	weights: {
		affordability: 0.4,
		location: 0.3,
		liveability: 0.3,
	},
	affordability: {
		priceWeight: 1.0, // 100% true cost percentile (rent + heating)
		epcWeight: 0.0, // EPC captured via true cost; avoid double-counting
		heatingCosts: {
			A: 30,
			B: 45,
			C: 70,
			D: 100,
			E: 400,
			F: 450,
			G: 500,
		},
	},
	location: {
		stationWeight: 0.25,
		broadbandWeight: 0.25,
		priorityWeight: 0.3,
		imdWeight: 0.12,
		crimeWeight: 0.08,
	},
	liveability: {
		gardenWeight: 0.45,
		heatingWeight: 0.3,
		propertyTypeWeight: 0.25,
		garden: {
			private: 100,
			shared: 40,
			none: 0,
		},
		heating: {
			gas: 100,
			electric: 60,
			unknown: 30,
		},
		propertyType: {
			detached: 95,
			house: 95,
			'semi-detached': 90,
			terraced: 85,
			cottage: 85,
			bungalow: 80,
			flat: 65,
			apartment: 65,
			studio: 40,
		},
	},
	penalties: {
		epcF: 0.0,
		epcG: 0.0,
		noGarden: 0.5,
		noPets: 0.4,
		missingDataPenalty: 0.95,
		gardenRequired: false, // Set by applySearchScoringSync based on mustHave
	},
	regionPriority: {
		York: 95,
		Durham: 90,
		Stamford: 90,
		Brighton: 85,
		Harrogate: 85,
		Newcastle: 80,
		Liverpool: 80,
		Morpeth: 80,
		Lancaster: 75,
		Folkestone: 75,
		Leicester: 75,
		Nottingham: 70,
		Sheffield: 70,
		Swansea: 70,
		Leeds: 65,
		Manchester: 65,
	},
};

// =============================================================================
// CONFIG CACHE
// =============================================================================

let cachedConfig: Config | null = null;

/**
 * Reset cached config (for testing)
 */
export function resetConfigCache(): void {
	cachedConfig = null;
}

// =============================================================================
// LOADER FUNCTIONS
// =============================================================================

/**
 * Auto-adjust scoring based on search filters
 *
 * Sets gardenRequired based on whether garden is in mustHave.
 * The liveability garden scoring still applies (private > shared > none),
 * but the noGarden penalty only applies when gardenRequired is true.
 */
function applySearchScoringSync(config: Config): Config {
	const gardenRequired = config.search.filters.mustHave.includes('garden');

	return {
		...config,
		scoring: {
			...config.scoring,
			penalties: {
				...config.scoring.penalties,
				gardenRequired,
			},
		},
	};
}

/**
 * Load and validate configuration from a TOML file
 *
 * @param configPath - Absolute path to config.toml
 * @throws {Error} if file cannot be read
 * @throws {ZodError} if config is invalid
 */
export async function loadConfig(configPath: string): Promise<Config> {
	if (cachedConfig) {
		return cachedConfig;
	}

	const file = Bun.file(configPath);
	const text = await file.text();
	const TOML = await import('smol-toml');
	const raw = TOML.parse(text);
	const parsed = ConfigSchema.parse(raw);

	// Sync scoring penalties with search filters
	cachedConfig = applySearchScoringSync(parsed);
	return cachedConfig;
}

/**
 * Parse scoring config from raw TOML object
 *
 * Standalone scoring config loader for use without full config.
 */
export function parseScoringConfig(rawConfig: Record<string, unknown>): ScoringConfig {
	const scoring = rawConfig['scoring'];

	if (!scoring) {
		return DEFAULT_SCORING_CONFIG;
	}

	const result = ScoringConfigSchema.safeParse(scoring);

	if (!result.success) {
		log.cli.warn('Invalid scoring config, using defaults', {
			errors: result.error.issues.map((i) => `${i.path.join('.')}: ${i.message}`),
		});
		return DEFAULT_SCORING_CONFIG;
	}

	return result.data;
}

/**
 * Load scoring config from config.toml file
 *
 * Standalone loader when only scoring config is needed.
 */
export async function loadScoringConfig(configPath?: string): Promise<ScoringConfig> {
	const path = configPath ?? `${process.cwd()}/data/let.config.toml`;

	try {
		const file = Bun.file(path);
		const text = await file.text();
		const rawConfig = await import('bun').then((bun) => bun.TOML.parse(text));
		return parseScoringConfig(rawConfig as Record<string, unknown>);
	} catch (error) {
		log.cli.warn('Failed to load scoring config, using defaults', { path, error: String(error) });
		return DEFAULT_SCORING_CONFIG;
	}
}
