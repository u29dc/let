/**
 * Pipeline Stage 4: Score
 *
 * Scoring and ranking of listings based on configured preferences.
 * Exports the public API for scoring listings.
 */

import { createHash } from 'node:crypto';
import type { Listing } from '@let/core/schema';
import { log } from '@let/core/utils/logger';
import { parseScoringConfig } from '../../config/index.js';
import { calculateAssessedScore } from '../assess/index.js';
import { aggregateScores } from './aggregate.js';
import { calculateAffordability, calculateLiveability, calculateLocation } from './composites.js';
import { calculateConfidence } from './confidence.js';
import { extractRawFactors } from './factors/extract.js';
import { normalizeFactors } from './factors/normalize.js';
import { roundTo } from './math/basic.js';
import { buildPercentileContext, calculateStats } from './math/percentiles.js';
import { calculatePenalties } from './penalties.js';
import type { CompositeScores, PercentileContext, ScoreContextMetadata, ScoredListing, ScoreFactors, Scores, ScoringConfig, ScoringContext, ScoringResult } from './types.js';

// Re-export aggregation helpers
export {
	aggregateScores,
	calculateCompositeImpact,
	calculateRawScore,
	varianceAdaptiveAggregate,
	weightedArithmeticMean,
	weightedGeometricMean,
} from './aggregate.js';
// Re-export composites
export { calculateAffordability, calculateLiveability, calculateLocation, getAffordabilityBreakdown, getLiveabilityBreakdown, getLocationBreakdown } from './composites.js';
// Re-export confidence helpers
export { calculateConfidence, describeConfidence } from './confidence.js';
// Re-export factor extraction/normalization
export {
	detectGardenType,
	detectHeatingType,
	detectPetPolicy,
	extractRawFactors,
	extractRegionName,
	getNearestStationDistance,
} from './factors/extract.js';
export { normalizeFactors } from './factors/normalize.js';
// Re-export basic math
export { clamp, exponentialDecay, inverseLerp, lerp, roundTo, sigmoid, sigmoidThreshold } from './math/basic.js';
// Re-export percentile helpers
export { buildPercentileContext, calculatePercentile, calculateStats, percentileToLabel } from './math/percentiles.js';
// Re-export utility functions
export { broadbandUtility, epcBandToNumeric, floorAreaUtility, getHeatingCostEstimate, normalizePropertyType, stationProximityUtility } from './math/utilities.js';
// Re-export penalties
export { calculatePenalties, explainPenalties } from './penalties.js';
// Re-export region helpers
export { extractNameFromAddress, extractNameFromRegion } from './regions.js';
// Re-export types
export type {
	AffordabilityConfig,
	CompositeScores,
	CompositeWeights,
	ConfidenceMetadata,
	EpcBand,
	GardenType,
	HeatingType,
	LiveabilityConfig,
	LocationConfig,
	NormalizedFactors,
	PenaltyConfig,
	PenaltyMultipliers,
	PercentileContext,
	PetPolicy,
	RawFactors,
	ScoreContextMetadata,
	ScoredListing,
	ScoreFactors,
	Scores,
	ScoringConfig,
	ScoringContext,
	ScoringResult,
	StatsSummary,
} from './types.js';

// =============================================================================
// PUBLIC API
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
			E: 150,
			F: 200,
			G: 250,
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
		gardenRequired: false, // Set to true if garden in mustHave
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

function hashScoringConfig(config: ScoringConfig): string {
	const serialized = JSON.stringify(config);
	return createHash('sha256').update(serialized).digest('hex');
}

function buildScoreContextMetadata(config: ScoringConfig, percentiles: PercentileContext): ScoreContextMetadata {
	return {
		configHash: hashScoringConfig(config),
		percentiles: {
			prices: calculateStats(percentiles.prices),
			trueCosts: calculateStats(percentiles.trueCosts),
			floorAreas: calculateStats(percentiles.floorAreas),
			stationDistances: calculateStats(percentiles.stationDistances),
			crimeRates: calculateStats(percentiles.crimeRates),
		},
	};
}

/**
 * Score a single listing using pre-computed context
 *
 * @param listing - Single listing to score
 * @param context - Pre-computed scoring context
 * @returns Listing with scores attached
 */
export function scoreSingleListing(listing: Listing, context: ScoringContext): ScoredListing {
	const { config, percentiles, metadata } = context;

	const rawFactors = extractRawFactors(listing, Object.keys(config.regionPriority));
	const normalizedFactors = normalizeFactors(rawFactors, percentiles, config);

	const composites: CompositeScores = {
		affordability: calculateAffordability(normalizedFactors, config.affordability),
		location: calculateLocation(normalizedFactors, config.location),
		liveability: calculateLiveability(normalizedFactors, config.liveability),
	};

	const penalties = calculatePenalties(normalizedFactors, config.penalties);
	const confidence = calculateConfidence(normalizedFactors, config);
	const overall = aggregateScores(composites, config.weights, penalties, config.adaptiveness, config.adaptivenessFactor);

	const factors: ScoreFactors = {
		monthlyRent: normalizedFactors.monthlyRent,
		pricePercentile: roundTo(normalizedFactors.pricePercentile, 1),
		floorAreaSqm: normalizedFactors.floorAreaSqm,
		floorAreaPercentile: normalizedFactors.floorAreaPercentile !== null ? roundTo(normalizedFactors.floorAreaPercentile, 1) : null,
		epcBand: normalizedFactors.epcBand,
		epcNumeric: normalizedFactors.epcNumeric,
		trueMonthlyCost: roundTo(normalizedFactors.trueMonthlyCost, 0),
		trueCostPercentile: roundTo(normalizedFactors.trueCostPercentile, 1),
		stationMiles: normalizedFactors.stationMiles,
		stationPercentile: normalizedFactors.stationPercentile !== null ? roundTo(normalizedFactors.stationPercentile, 1) : null,
		gigabitPct: normalizedFactors.gigabitPct,
		regionName: normalizedFactors.regionName,
		priorityScore: normalizedFactors.priorityScore,
		gardenType: normalizedFactors.gardenType,
		heatingType: normalizedFactors.heatingType,
		petPolicy: normalizedFactors.petPolicy,
		propertyType: normalizedFactors.propertyType,
		bedrooms: normalizedFactors.bedrooms,
		imdDecile: normalizedFactors.imdDecile ?? null,
		crimeRatePer1k: normalizedFactors.crimeRatePer1k ?? null,
		crimeRatePercentile: normalizedFactors.crimeRatePercentile ?? null,
	};

	const scores: Scores = {
		_overall: overall,
		confidence: roundTo(confidence.score, 2),
		affordability: roundTo(composites.affordability * 100, 0),
		location: roundTo(composites.location * 100, 0),
		liveability: roundTo(composites.liveability * 100, 0),
		factors,
		penalties,
		context: metadata,
	};

	return {
		...listing,
		scores,
	};
}

/**
 * Build scoring context from existing listings
 *
 * @param listings - Existing listings to derive context from
 * @param config - Scoring configuration
 * @returns Scoring context for use with scoreSingleListing
 */
export function buildScoringContext(listings: Listing[], config: ScoringConfig): ScoringContext {
	const percentiles = buildPercentileContext(listings, config);
	const metadata = buildScoreContextMetadata(config, percentiles);
	return { config, percentiles, metadata };
}

/**
 * Score a batch of listings
 *
 * This is the main entry point for batch scoring.
 * Computes percentiles across the entire dataset for relative scoring.
 *
 * @param listings - Array of listings to score
 * @param config - Scoring configuration
 * @returns Scored listings sorted by _overall descending
 */
export function scoreListings(listings: Listing[], config: ScoringConfig): ScoringResult {
	if (listings.length === 0) {
		const emptyPercentiles = { prices: [], trueCosts: [], floorAreas: [], stationDistances: [], crimeRates: [] };
		const metadata = buildScoreContextMetadata(config, emptyPercentiles);
		return {
			listings: [],
			context: {
				config,
				percentiles: emptyPercentiles,
				metadata,
			},
			stats: { total: 0, scored: 0, avgScore: 0, avgConfidence: 0 },
		};
	}

	const percentiles = buildPercentileContext(listings, config);
	const metadata = buildScoreContextMetadata(config, percentiles);
	const context: ScoringContext = { config, percentiles, metadata };

	const scoredListings: ScoredListing[] = listings.map((listing) => scoreSingleListing(listing, context));
	scoredListings.sort((a, b) => b.scores._overall - a.scores._overall);

	const totalScore = scoredListings.reduce((sum, l) => sum + l.scores._overall, 0);
	const totalConfidence = scoredListings.reduce((sum, l) => sum + l.scores.confidence, 0);

	log.score.success('Listings scored', {
		count: scoredListings.length,
		avgScore: roundTo(totalScore / scoredListings.length, 1),
	});

	return {
		listings: scoredListings,
		context,
		stats: {
			total: listings.length,
			scored: scoredListings.length,
			avgScore: roundTo(totalScore / scoredListings.length, 1),
			avgConfidence: roundTo(totalConfidence / scoredListings.length, 2),
		},
	};
}

/**
 * Score listings using raw config object (for CLI integration)
 *
 * @param listings - Listings to score
 * @param rawConfig - Raw config object from TOML parsing
 * @returns Scored listings
 */
export function scoreListingsWithConfig(listings: Listing[], rawConfig: Record<string, unknown>): ScoredListing[] {
	if (listings.length === 0) return [];

	const scoringConfig = parseScoringConfig(rawConfig);
	const result = scoreListings(listings, scoringConfig);

	return result.listings;
}

/**
 * Recalculate assessedScore for all assessed listings after re-scoring
 *
 * When listings are re-scored (percentiles shift), assessedScore becomes stale.
 * This function updates assessedScore = scores._overall + assessment adjustment
 * for all listings that have an assessment.
 *
 * @param listings - Listings to update (only those with both scores and assessment are affected)
 */
export function recalcAssessedScores(listings: Listing[]): void {
	let updated = 0;
	for (const listing of listings) {
		if (listing.assessment && listing.scores) {
			listing.assessedScore = calculateAssessedScore(listing.scores._overall, listing.assessment);
			updated++;
		}
	}
	if (updated > 0) {
		log.score.info('Recalculated assessed scores', { updated });
	}
}
