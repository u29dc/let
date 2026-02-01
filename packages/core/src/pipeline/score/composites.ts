/**
 * Composite score calculations
 *
 * Each composite aggregates related factors into a 0-1 score:
 * - Affordability: price percentile, EPC efficiency (running costs)
 * - Location: station proximity, broadband, region priority
 * - Liveability: garden, heating, property type
 */

import { weightedArithmeticMean } from './aggregate.js';
import { broadbandUtility, imdDecileToScore, stationProximityUtility } from './math/utilities.js';
import type { AffordabilityConfig, GardenType, HeatingType, LiveabilityConfig, LocationConfig, NormalizedFactors } from './types.js';

// =============================================================================
// AFFORDABILITY
// =============================================================================

/**
 * Calculate affordability composite score
 *
 * Combines:
 * - True monthly cost (rent + estimated heating) percentile
 * - EPC numeric score (energy efficiency)
 *
 * Default values for missing data:
 * - epcScore: 0.5 when EPC rating unknown (neutral)
 *
 * Note: Floor area removed from scoring - if searching for N-bed,
 * the size will be reasonable for that bedroom count.
 *
 * @param factors - Normalized factors with percentiles
 * @param config - Affordability configuration
 * @returns Score 0-1 (multiply by 100 for display)
 */
export function calculateAffordability(factors: NormalizedFactors, config: AffordabilityConfig): number {
	const trueCostScore = factors.trueCostPercentile / 100;

	// Default: 0.5 when EPC rating unknown (neutral score)
	const epcScore = factors.epcNumeric !== null ? factors.epcNumeric / 100 : 0.5;

	const composite = weightedArithmeticMean([
		[trueCostScore, config.priceWeight],
		[epcScore, config.epcWeight],
	]);

	return composite;
}

/**
 * Get affordability breakdown for display
 */
export function getAffordabilityBreakdown(
	factors: NormalizedFactors,
	config: AffordabilityConfig,
): {
	trueCostScore: number;
	epcScore: number;
	composite: number;
} {
	const trueCostScore = factors.trueCostPercentile / 100;
	const epcScore = factors.epcNumeric !== null ? factors.epcNumeric / 100 : 0.5;

	const composite = weightedArithmeticMean([
		[trueCostScore, config.priceWeight],
		[epcScore, config.epcWeight],
	]);

	return {
		trueCostScore,
		epcScore,
		composite,
	};
}

// =============================================================================
// LOCATION
// =============================================================================

/**
 * Calculate location composite score
 *
 * Combines:
 * - Station proximity (exponential decay beyond threshold)
 * - Broadband availability (sigmoid threshold)
 * - Region priority (pre-configured scores)
 * - IMD decile (lower deprivation = higher score)
 * - Crime rate percentile (lower crime = higher score)
 *
 * Default values for missing data:
 * - stationScore: 0.5 when no station data (neutral)
 * - broadbandScore: 0.5 when no broadband data (neutral)
 * - priorityScore: 0.7 when region not in config (neutral default)
 * - imdScore: null when IMD decile unavailable (weight redistributed)
 * - crimeScore: null when crime rate unavailable (weight redistributed)
 *
 * @param factors - Normalized factors with percentiles
 * @param config - Location configuration
 * @returns Score 0-1 (multiply by 100 for display)
 */
export function calculateLocation(factors: NormalizedFactors, config: LocationConfig): number {
	// Default: 0.5 when station distance unknown (neutral)
	let stationScore: number;
	if (factors.stationMiles !== null) {
		stationScore = stationProximityUtility(factors.stationMiles);
	} else {
		stationScore = 0.5;
	}

	// Default: 0.5 when broadband data unavailable (neutral)
	let broadbandScore: number;
	if (factors.gigabitPct !== null) {
		broadbandScore = broadbandUtility(factors.gigabitPct);
	} else {
		broadbandScore = 0.5;
	}

	let priorityScore: number | null = null;
	if (factors.priorityScore !== null) {
		priorityScore = factors.priorityScore / 100;
	}

	let imdScore: number | null = null;
	if (factors.imdDecile !== null) {
		imdScore = imdDecileToScore(factors.imdDecile);
	}

	let crimeScore: number | null = null;
	if (factors.crimeRatePercentile !== null) {
		crimeScore = factors.crimeRatePercentile / 100;
	}

	const composite = weightedArithmeticMean([
		[stationScore, config.stationWeight],
		[broadbandScore, config.broadbandWeight],
		[priorityScore ?? 0, priorityScore === null ? 0 : config.priorityWeight],
		[imdScore ?? 0, imdScore === null ? 0 : config.imdWeight],
		[crimeScore ?? 0, crimeScore === null ? 0 : config.crimeWeight],
	]);

	return composite;
}

/**
 * Get location breakdown for display
 */
export function getLocationBreakdown(
	factors: NormalizedFactors,
	config: LocationConfig,
): {
	stationScore: number;
	broadbandScore: number;
	priorityScore: number;
	imdScore: number | null;
	crimeScore: number | null;
	composite: number;
} {
	let stationScore: number;
	if (factors.stationMiles !== null) {
		stationScore = stationProximityUtility(factors.stationMiles);
	} else {
		stationScore = 0.5;
	}

	let broadbandScore: number;
	if (factors.gigabitPct !== null) {
		broadbandScore = broadbandUtility(factors.gigabitPct);
	} else {
		broadbandScore = 0.5;
	}

	let priorityScore: number;
	if (factors.priorityScore !== null) {
		priorityScore = factors.priorityScore / 100;
	} else {
		priorityScore = 0.7;
	}

	let imdScore: number | null = null;
	if (factors.imdDecile !== null) {
		imdScore = imdDecileToScore(factors.imdDecile);
	}

	let crimeScore: number | null = null;
	if (factors.crimeRatePercentile !== null) {
		crimeScore = factors.crimeRatePercentile / 100;
	}

	const composite = weightedArithmeticMean([
		[stationScore, config.stationWeight],
		[broadbandScore, config.broadbandWeight],
		[priorityScore, config.priorityWeight],
		[imdScore ?? 0, imdScore === null ? 0 : config.imdWeight],
		[crimeScore ?? 0, crimeScore === null ? 0 : config.crimeWeight],
	]);

	return {
		stationScore,
		broadbandScore,
		priorityScore,
		imdScore,
		crimeScore,
		composite,
	};
}

// =============================================================================
// LIVEABILITY
// =============================================================================

/**
 * Get garden score from config
 */
function getGardenScore(gardenType: GardenType, config: LiveabilityConfig): number {
	const score = config.garden[gardenType];
	return score / 100;
}

/**
 * Get heating score from config
 */
function getHeatingScore(heatingType: HeatingType, config: LiveabilityConfig): number {
	const score = config.heating[heatingType];
	return score / 100;
}

/**
 * Get property type score from config
 */
function getPropertyTypeScore(propertyType: string | null, config: LiveabilityConfig): number {
	if (!propertyType) return 0.7;

	const score = config.propertyType[propertyType];
	if (score !== undefined) {
		return score / 100;
	}

	const lowerType = propertyType.toLowerCase();

	if (lowerType.includes('house') || lowerType.includes('detach') || lowerType.includes('cottage')) {
		return 0.9;
	}

	if (lowerType.includes('flat') || lowerType.includes('apartment') || lowerType.includes('studio')) {
		return 0.6;
	}

	return 0.7;
}

/**
 * Calculate liveability composite score
 *
 * Combines:
 * - Garden type (private > shared > none)
 * - Heating type (gas > electric > unknown)
 * - Property type (house types > flats)
 *
 * @param factors - Normalized factors with percentiles
 * @param config - Liveability configuration
 * @returns Score 0-1 (multiply by 100 for display)
 */
export function calculateLiveability(factors: NormalizedFactors, config: LiveabilityConfig): number {
	const gardenScore = getGardenScore(factors.gardenType, config);
	const heatingScore = getHeatingScore(factors.heatingType, config);
	const propertyTypeScore = getPropertyTypeScore(factors.propertyType, config);

	const composite = weightedArithmeticMean([
		[gardenScore, config.gardenWeight],
		[heatingScore, config.heatingWeight],
		[propertyTypeScore, config.propertyTypeWeight],
	]);

	return composite;
}

/**
 * Get liveability breakdown for display
 */
export function getLiveabilityBreakdown(
	factors: NormalizedFactors,
	config: LiveabilityConfig,
): {
	gardenScore: number;
	heatingScore: number;
	propertyTypeScore: number;
	composite: number;
} {
	const gardenScore = getGardenScore(factors.gardenType, config);
	const heatingScore = getHeatingScore(factors.heatingType, config);
	const propertyTypeScore = getPropertyTypeScore(factors.propertyType, config);

	const composite = weightedArithmeticMean([
		[gardenScore, config.gardenWeight],
		[heatingScore, config.heatingWeight],
		[propertyTypeScore, config.propertyTypeWeight],
	]);

	return {
		gardenScore,
		heatingScore,
		propertyTypeScore,
		composite,
	};
}
