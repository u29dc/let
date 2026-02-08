/**
 * Penalty multipliers for deal-breaker conditions
 *
 * Instead of hard exclusion, severe deficiencies apply multiplicative
 * penalties that significantly reduce the final score while keeping
 * properties comparable in a single ranking.
 *
 * Penalties are fully configurable via scoring config.
 * Defaults are set in config and may be disabled (0.0) unless explicitly enabled.
 */

import type { NormalizedFactors, PenaltyConfig, PenaltyMultipliers } from './types.js';

/**
 * Calculate EPC penalty multiplier
 */
function calculateEpcPenalty(epcBand: string | null, config: PenaltyConfig): number {
	if (!epcBand) {
		return 1.0;
	}

	const band = epcBand.toUpperCase();

	if (band === 'G') {
		return config.epcG;
	}

	if (band === 'F') {
		return config.epcF;
	}

	return 1.0;
}

/**
 * Calculate garden penalty multiplier
 * Only applies when gardenRequired is true (garden in mustHave)
 */
function calculateGardenPenalty(gardenType: 'private' | 'shared' | 'none', config: PenaltyConfig): number {
	// Skip penalty if garden not required
	if (!config.gardenRequired) {
		return 1.0;
	}

	if (gardenType === 'none') {
		return config.noGarden;
	}

	return 1.0;
}

/**
 * Calculate pet policy penalty multiplier
 */
function calculatePetsPenalty(petPolicy: 'yes' | 'no' | 'unknown', config: PenaltyConfig): number {
	if (petPolicy === 'no') {
		return config.noPets;
	}

	return 1.0;
}

/**
 * Calculate deprivation penalty multiplier
 * Applies when IMD decile is at or below the configured threshold (most deprived areas)
 */
function calculateDeprivationPenalty(imdDecile: number | null, config: PenaltyConfig): number {
	if (imdDecile === null || imdDecile > config.deprivationThreshold) return 1.0;
	return config.deprivation;
}

/**
 * Calculate high crime penalty multiplier
 * Applies when crime rate per 1k exceeds the configured threshold
 */
function calculateHighCrimePenalty(crimeRatePer1k: number | null, config: PenaltyConfig): number {
	if (crimeRatePer1k === null || crimeRatePer1k <= config.highCrimeThreshold) return 1.0;
	return config.highCrime;
}

/**
 * Calculate missing-data penalty multiplier
 */
function calculateMissingDataPenalty(factors: NormalizedFactors, config: PenaltyConfig): number {
	const penalty = config.missingDataPenalty;
	if (!Number.isFinite(penalty) || penalty >= 1) return 1.0;

	let missingCount = 0;
	if (factors.epcBand === null) missingCount++;
	if (factors.stationMiles === null) missingCount++;
	if (factors.gigabitPct === null) missingCount++;
	if (factors.priorityScore === null) missingCount++;
	if (factors.imdDecile === null) missingCount++;
	if (factors.crimeRatePer1k === null) missingCount++;

	if (missingCount === 0) return 1.0;
	return penalty ** missingCount;
}

/**
 * Calculate all penalty multipliers for a listing
 *
 * Returns individual penalties and combined multiplier
 */
export function calculatePenalties(factors: NormalizedFactors, config: PenaltyConfig): PenaltyMultipliers {
	const epc = calculateEpcPenalty(factors.epcBand, config);
	const garden = calculateGardenPenalty(factors.gardenType, config);
	const pets = calculatePetsPenalty(factors.petPolicy, config);
	const deprivation = calculateDeprivationPenalty(factors.imdDecile, config);
	const highCrime = calculateHighCrimePenalty(factors.crimeRatePer1k, config);
	const missing = calculateMissingDataPenalty(factors, config);

	const combined = epc * garden * pets * deprivation * highCrime * missing;

	return {
		epc,
		garden,
		pets,
		deprivation,
		highCrime,
		combined,
	};
}

/**
 * Get human-readable explanation of penalties
 */
export function explainPenalties(penalties: PenaltyMultipliers): string[] {
	const explanations: string[] = [];

	if (penalties.epc < 1.0) {
		if (penalties.epc <= 0.1) {
			explanations.push(`EPC G: ${Math.round((1 - penalties.epc) * 100)}% penalty (essentially uninhabitable)`);
		} else if (penalties.epc <= 0.3) {
			explanations.push(`EPC F: ${Math.round((1 - penalties.epc) * 100)}% penalty (very high heating costs)`);
		}
	}

	if (penalties.garden < 1.0) {
		explanations.push(`No garden: ${Math.round((1 - penalties.garden) * 100)}% penalty (garden required)`);
	}

	if (penalties.pets < 1.0) {
		explanations.push(`No pets allowed: ${Math.round((1 - penalties.pets) * 100)}% penalty (need dog-friendly)`);
	}

	if (penalties.deprivation < 1.0) {
		explanations.push(`High deprivation area: ${Math.round((1 - penalties.deprivation) * 100)}% penalty (IMD decile in most deprived)`);
	}

	if (penalties.highCrime < 1.0) {
		explanations.push(`High crime area: ${Math.round((1 - penalties.highCrime) * 100)}% penalty (crime rate exceeds threshold)`);
	}

	return explanations;
}
