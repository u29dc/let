/**
 * Pipeline Stage 4: Score
 *
 * Scoring and ranking of listings based on configured preferences.
 * Exports the public API for scoring listings.
 */

import { createHash } from 'node:crypto';
import type { Listing } from '@let/core/schema';
import { log } from '@let/core/utils/logger';
import { DEFAULT_SCORING_CONFIG, parseScoringConfig } from '../../config/index.js';
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

export { DEFAULT_SCORING_CONFIG };

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
