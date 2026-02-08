/**
 * Type definitions for the scoring system
 *
 * The scoring system uses:
 * - Variance-adaptive aggregation (geometric for consistency, arithmetic for compensation)
 * - Percentile-based scoring (relative to dataset)
 * - True cost modeling (rent + heating estimates)
 * - Multiplicative penalties for deal-breakers
 */

import type { Listing } from '@let/core/schema';

// =============================================================================
// CONFIGURATION TYPES
// =============================================================================

/**
 * Scoring configuration loaded from config.toml
 */
export interface ScoringConfig {
	/** How aggressively variance allows compensation (1.0=conservative, 2.0=balanced, 4.0=aggressive) */
	adaptiveness: number;
	/** Scalar applied to adaptiveness for sigmoid steepness (exposed to avoid hidden constants) */
	adaptivenessFactor: number;
	weights: CompositeWeights;
	affordability: AffordabilityConfig;
	location: LocationConfig;
	liveability: LiveabilityConfig;
	penalties: PenaltyConfig;
	regionPriority: Record<string, number>;
}

export interface CompositeWeights {
	affordability: number;
	location: number;
	liveability: number;
}

export interface AffordabilityConfig {
	priceWeight: number;
	epcWeight: number;
	heatingCosts: Record<EpcBand, number>;
}

export interface LocationConfig {
	stationWeight: number;
	broadbandWeight: number;
	priorityWeight: number;
	imdWeight: number;
	crimeWeight: number;
}

export interface LiveabilityConfig {
	gardenWeight: number;
	heatingWeight: number;
	propertyTypeWeight: number;
	garden: Record<GardenType, number>;
	heating: Record<HeatingType, number>;
	propertyType: Record<string, number>;
}

export interface PenaltyConfig {
	epcF: number;
	epcG: number;
	noGarden: number;
	noPets: number;
	deprivation: number; // Multiplier (0-1), applied when IMD decile <= deprivationThreshold
	deprivationThreshold: number; // IMD decile cutoff (1-10 scale)
	highCrime: number; // Multiplier (0-1), applied when crime rate > highCrimeThreshold
	highCrimeThreshold: number; // Crime rate per 1k population cutoff
	missingDataPenalty: number;
	gardenRequired: boolean; // Only apply noGarden penalty if true (garden in mustHave)
}

// =============================================================================
// FACTOR TYPES
// =============================================================================

export type EpcBand = 'A' | 'B' | 'C' | 'D' | 'E' | 'F' | 'G';
export type GardenType = 'private' | 'shared' | 'none';
export type HeatingType = 'gas' | 'electric' | 'unknown';
export type PetPolicy = 'yes' | 'no' | 'unknown';

/**
 * Raw factors extracted directly from listing data
 */
export interface RawFactors {
	monthlyRent: number;
	floorAreaSqm: number | null;
	epcBand: string | null;
	bedrooms: number;
	stationMiles: number | null;
	gigabitPct: number | null;
	regionName: string | null;
	gardenType: GardenType;
	heatingType: HeatingType;
	petPolicy: PetPolicy;
	propertyType: string | null;
	imdDecile: number | null;
	crimeRatePer1k: number | null;
}

/**
 * Percentile context computed from the full dataset
 */
export interface PercentileContext {
	prices: number[];
	trueCosts: number[];
	floorAreas: number[];
	stationDistances: number[];
	crimeRates: number[];
}

export interface StatsSummary {
	min: number;
	max: number;
	mean: number;
	median: number;
	stdDev: number;
}

export interface ScoreContextMetadata {
	configHash: string;
	percentiles: {
		prices: StatsSummary;
		trueCosts: StatsSummary;
		floorAreas: StatsSummary;
		stationDistances: StatsSummary;
		crimeRates: StatsSummary;
	};
}

/**
 * Normalized factors with percentile ranks and derived metrics
 */
export interface NormalizedFactors extends RawFactors {
	pricePercentile: number;
	trueCostPercentile: number;
	floorAreaPercentile: number | null;
	stationPercentile: number | null;
	trueMonthlyCost: number;
	epcNumeric: number | null;
	priorityScore: number | null;
	crimeRatePercentile: number | null;
}

// =============================================================================
// COMPOSITE TYPES
// =============================================================================

/**
 * Composite scores (each 0-1 internally, displayed as 0-100)
 */
export interface CompositeScores {
	affordability: number;
	location: number;
	liveability: number;
}

/**
 * Penalty multipliers (each 0-1, combined multiplicatively)
 */
export interface PenaltyMultipliers {
	epc: number;
	garden: number;
	pets: number;
	deprivation: number;
	highCrime: number;
	combined: number;
}

// =============================================================================
// CONFIDENCE TYPES
// =============================================================================

/**
 * Data completeness and confidence metadata
 */
export interface ConfidenceMetadata {
	score: number;
	availableFactors: string[];
	missingFactors: string[];
	quality: 'high' | 'medium' | 'low';
}

// =============================================================================
// OUTPUT TYPES
// =============================================================================

/**
 * Factors object stored in listing scores
 */
export interface ScoreFactors {
	monthlyRent: number;
	pricePercentile: number;
	floorAreaSqm: number | null;
	floorAreaPercentile: number | null;
	epcBand: string | null;
	epcNumeric: number | null;
	trueMonthlyCost: number;
	trueCostPercentile: number;
	stationMiles: number | null;
	stationPercentile: number | null;
	gigabitPct: number | null;
	regionName: string | null;
	priorityScore: number | null;
	gardenType: GardenType;
	heatingType: HeatingType;
	petPolicy: PetPolicy;
	propertyType: string | null;
	bedrooms: number;
	imdDecile: number | null;
	crimeRatePer1k: number | null;
	crimeRatePercentile: number | null;
}

/**
 * Final scores object stored on listing
 */
export interface Scores {
	_overall: number;
	confidence: number;
	affordability: number;
	location: number;
	liveability: number;
	factors: ScoreFactors;
	penalties: PenaltyMultipliers;
	context: ScoreContextMetadata;
}

/**
 * Listing with scores attached
 */
export interface ScoredListing extends Listing {
	scores: Scores;
}

// =============================================================================
// FUNCTION SIGNATURES
// =============================================================================

/**
 * Context needed for scoring (config + percentiles)
 */
export interface ScoringContext {
	config: ScoringConfig;
	percentiles: PercentileContext;
	metadata: ScoreContextMetadata;
}

/**
 * Result of scoring a batch of listings
 */
export interface ScoringResult {
	listings: ScoredListing[];
	context: ScoringContext;
	stats: {
		total: number;
		scored: number;
		avgScore: number;
		avgConfidence: number;
	};
}
