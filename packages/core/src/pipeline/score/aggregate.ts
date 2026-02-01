/**
 * Score aggregation helpers
 */

import { clamp, sigmoid } from './math/basic.js';
import type { CompositeScores, CompositeWeights, PenaltyMultipliers } from './types.js';

/**
 * Weighted Arithmetic Mean
 */
export function weightedArithmeticMean(values: Array<[number, number]>): number {
	const nonZeroWeights = values.filter(([_, w]) => w > 0);

	if (nonZeroWeights.length === 0) return 0;

	const totalWeight = nonZeroWeights.reduce((sum, [_, w]) => sum + w, 0);
	const weightedSum = nonZeroWeights.reduce((sum, [v, w]) => sum + v * w, 0);

	return weightedSum / totalWeight;
}

/**
 * Weighted Geometric Mean
 */
export function weightedGeometricMean(values: Array<[number, number]>): number {
	const nonZeroWeights = values.filter(([_, w]) => w > 0);

	if (nonZeroWeights.length === 0) return 0;

	const totalWeight = nonZeroWeights.reduce((sum, [_, w]) => sum + w, 0);

	if (nonZeroWeights.some(([v, _]) => v <= 0)) {
		const minValue = Math.min(...nonZeroWeights.map(([v, _]) => v));
		if (minValue <= 0) {
			return 0.01;
		}
	}

	let logSum = 0;
	for (const [value, weight] of nonZeroWeights) {
		logSum += (weight / totalWeight) * Math.log(Math.max(0.001, value));
	}

	return Math.exp(logSum);
}

/**
 * Variance-Adaptive Aggregate
 */
export function varianceAdaptiveAggregate(values: Array<[number, number]>, adaptiveness = 2.0, center = 0.3, adaptivenessFactor = 10): number {
	const geoMean = weightedGeometricMean(values);
	const arithMean = weightedArithmeticMean(values);

	const scores = values.filter(([_, w]) => w > 0).map(([s, _]) => s);

	if (scores.length === 0) return 0;

	const mean = scores.reduce((a, b) => a + b, 0) / scores.length;
	if (mean === 0) return 0;

	const variance = scores.reduce((sum, s) => sum + (s - mean) ** 2, 0) / scores.length;
	const stdDev = Math.sqrt(variance);
	const cv = stdDev / mean;

	const alpha = sigmoid((cv - center) * adaptiveness * adaptivenessFactor);

	return alpha * arithMean + (1 - alpha) * geoMean;
}

/**
 * Normalize composite weights to sum to 1.0 with non-negative values.
 * Falls back to equal weights if invalid.
 */
function normalizeCompositeWeights(weights: CompositeWeights): CompositeWeights {
	const safe = {
		affordability: Math.max(0, weights.affordability),
		location: Math.max(0, weights.location),
		liveability: Math.max(0, weights.liveability),
	};
	const total = safe.affordability + safe.location + safe.liveability;
	if (!Number.isFinite(total) || total <= 0) {
		return { affordability: 1 / 3, location: 1 / 3, liveability: 1 / 3 };
	}
	return {
		affordability: safe.affordability / total,
		location: safe.location / total,
		liveability: safe.liveability / total,
	};
}

/**
 * Aggregate composite scores into overall score using variance-adaptive aggregation.
 */
export function aggregateScores(composites: CompositeScores, weights: CompositeWeights, penalties: PenaltyMultipliers, adaptiveness = 2.0, adaptivenessFactor = 10): number {
	const normalized = normalizeCompositeWeights(weights);
	const values: Array<[number, number]> = [
		[composites.affordability, normalized.affordability],
		[composites.location, normalized.location],
		[composites.liveability, normalized.liveability],
	];

	const rawScore = varianceAdaptiveAggregate(values, adaptiveness, 0.3, adaptivenessFactor);
	const penalizedScore = rawScore * penalties.combined;
	const finalScore = clamp(Math.round(penalizedScore * 100), 0, 100);

	return finalScore;
}

/**
 * Calculate what the score would be without penalties (using variance-adaptive aggregation)
 */
export function calculateRawScore(composites: CompositeScores, weights: CompositeWeights, adaptiveness = 2.0, adaptivenessFactor = 10): number {
	const normalized = normalizeCompositeWeights(weights);
	const values: Array<[number, number]> = [
		[composites.affordability, normalized.affordability],
		[composites.location, normalized.location],
		[composites.liveability, normalized.liveability],
	];

	const rawScore = varianceAdaptiveAggregate(values, adaptiveness, 0.3, adaptivenessFactor);
	return clamp(Math.round(rawScore * 100), 0, 100);
}

/**
 * Calculate score impact of each composite
 */
export function calculateCompositeImpact(
	composites: CompositeScores,
	_weights: CompositeWeights,
): {
	affordability: { score: number; impact: 'positive' | 'neutral' | 'negative' };
	location: { score: number; impact: 'positive' | 'neutral' | 'negative' };
	liveability: { score: number; impact: 'positive' | 'neutral' | 'negative' };
} {
	const threshold = 0.6;

	function getImpact(score: number): 'positive' | 'neutral' | 'negative' {
		if (score >= 0.7) return 'positive';
		if (score >= threshold) return 'neutral';
		return 'negative';
	}

	return {
		affordability: {
			score: Math.round(composites.affordability * 100),
			impact: getImpact(composites.affordability),
		},
		location: {
			score: Math.round(composites.location * 100),
			impact: getImpact(composites.location),
		},
		liveability: {
			score: Math.round(composites.liveability * 100),
			impact: getImpact(composites.liveability),
		},
	};
}
