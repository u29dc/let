/**
 * Confidence scoring based on data completeness
 */

import type { ConfidenceMetadata, NormalizedFactors, ScoringConfig } from './types.js';

const CONFIDENCE_FACTORS = ['price', 'epc', 'station', 'broadband', 'priority', 'imd', 'crime', 'garden', 'heating', 'propertyType'] as const;

type FactorName = (typeof CONFIDENCE_FACTORS)[number];

function buildConfidenceWeights(config: ScoringConfig): Record<FactorName, number> {
	return {
		price: config.weights.affordability * config.affordability.priceWeight,
		epc: config.weights.affordability * config.affordability.epcWeight,
		station: config.weights.location * config.location.stationWeight,
		broadband: config.weights.location * config.location.broadbandWeight,
		priority: config.weights.location * config.location.priorityWeight,
		imd: config.weights.location * config.location.imdWeight,
		crime: config.weights.location * config.location.crimeWeight,
		garden: config.weights.liveability * config.liveability.gardenWeight,
		heating: config.weights.liveability * config.liveability.heatingWeight,
		propertyType: config.weights.liveability * config.liveability.propertyTypeWeight,
	};
}

interface FactorCheck {
	name: FactorName;
	isPresent: boolean;
	partialCredit?: number;
}

function checkFactors(factors: NormalizedFactors): FactorCheck[] {
	return [
		{ name: 'price', isPresent: true },
		{ name: 'epc', isPresent: factors.epcBand !== null },
		{ name: 'garden', isPresent: true, partialCredit: factors.gardenType !== 'none' ? 1 : 0.5 },
		{ name: 'station', isPresent: factors.stationMiles !== null },
		{ name: 'broadband', isPresent: factors.gigabitPct !== null },
		{ name: 'heating', isPresent: true, partialCredit: factors.heatingType !== 'unknown' ? 1 : 0.5 },
		{ name: 'propertyType', isPresent: factors.propertyType !== null },
		{ name: 'priority', isPresent: factors.priorityScore !== null },
		{ name: 'imd', isPresent: factors.imdDecile !== null },
		{ name: 'crime', isPresent: factors.crimeRatePer1k !== null },
	];
}

function getQualityLevel(score: number): 'high' | 'medium' | 'low' {
	if (score >= 0.85) return 'high';
	if (score >= 0.65) return 'medium';
	return 'low';
}

/** Calculate confidence score based on data completeness */
export function calculateConfidence(factors: NormalizedFactors, config: ScoringConfig): ConfidenceMetadata {
	const weights = buildConfidenceWeights(config);
	const checks = checkFactors(factors);
	const availableFactors: string[] = [];
	const missingFactors: string[] = [];
	let achievedWeight = 0;

	const maxWeight = Object.values(weights).reduce((a, b) => a + b, 0);

	for (const check of checks) {
		const weight = weights[check.name];
		if (check.isPresent) {
			availableFactors.push(check.name);
			achievedWeight += weight * (check.partialCredit ?? 1);
		} else {
			missingFactors.push(check.name);
		}
	}

	const score = achievedWeight / maxWeight;

	return {
		score,
		availableFactors,
		missingFactors,
		quality: getQualityLevel(score),
	};
}

/** Get human-readable confidence description */
export function describeConfidence(confidence: ConfidenceMetadata): string {
	const percent = Math.round(confidence.score * 100);

	if (confidence.quality === 'high') {
		return `High confidence (${percent}%) - all key data available`;
	}

	const missing = confidence.missingFactors.join(', ');
	const label = confidence.quality === 'medium' ? 'Medium' : 'Low';
	return `${label} confidence (${percent}%) - missing: ${missing}`;
}
