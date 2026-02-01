/**
 * Percentile and stats helpers
 */

import type { Listing } from '@let/core/schema';
import type { PercentileContext, ScoringConfig } from '../types.js';
import { getHeatingCostEstimate } from './utilities.js';

/** Build percentile context from a dataset of listings */
export function buildPercentileContext(listings: Listing[], config: ScoringConfig): PercentileContext {
	const prices = listings.map((l) => l.price).sort((a, b) => a - b);

	const trueCosts = listings
		.map((l) => {
			const heatingCost = getHeatingCostEstimate(l.epcRating ?? null, config.affordability.heatingCosts);
			return l.price + heatingCost;
		})
		.sort((a, b) => a - b);

	const floorAreas = listings
		.map((l) => l.floorAreaSqm)
		.filter((area): area is number => area !== null && area !== undefined)
		.sort((a, b) => a - b);

	const stationDistances = listings
		.map((l) => (l.nearestStations.length > 0 ? l.nearestStations[0]?.distance : null))
		.filter((d): d is number => d !== null && d !== undefined)
		.sort((a, b) => a - b);

	const crimeRates = listings
		.map((l) => l.area.crime.ratePer1k)
		.filter((rate): rate is number => rate !== null && rate !== undefined && Number.isFinite(rate))
		.sort((a, b) => a - b);

	return {
		prices,
		trueCosts,
		floorAreas,
		stationDistances,
		crimeRates,
	};
}

function handleSingleElement(value: number, singleValue: number, invert: boolean): number {
	if (value === singleValue) return 50;
	const isValueBetter = invert ? value < singleValue : value > singleValue;
	return isValueBetter ? 75 : 25;
}

function handleTwoElements(value: number, firstValue: number, secondValue: number, invert: boolean): number {
	if (firstValue === secondValue) return 50;
	if (value <= firstValue) return invert ? 100 : 0;
	if (value >= secondValue) return invert ? 0 : 100;
	return 50;
}

function findInsertionPosition(value: number, sortedArray: number[]): number {
	let low = 0;
	let high = sortedArray.length;

	while (low < high) {
		const mid = Math.floor((low + high) / 2);
		const midValue = sortedArray[mid];
		if (midValue === undefined || midValue < value) {
			low = mid + 1;
		} else {
			high = mid;
		}
	}

	return low;
}

/** Calculate percentile rank for a value within a sorted array */
export function calculatePercentile(value: number, sortedArray: number[], invert = false): number {
	if (sortedArray.length === 0) return 50;

	const firstValue = sortedArray[0];
	if (sortedArray.length === 1 && firstValue !== undefined) {
		return handleSingleElement(value, firstValue, invert);
	}
	if (sortedArray.length === 2) {
		const secondValue = sortedArray[1];
		if (secondValue === undefined || firstValue === undefined) {
			return 50;
		}
		return handleTwoElements(value, firstValue, secondValue, invert);
	}

	const position = findInsertionPosition(value, sortedArray);
	const percentile = (position / sortedArray.length) * 100;

	return invert ? 100 - percentile : percentile;
}

/** Get descriptive label for a percentile */
export function percentileToLabel(percentile: number): string {
	if (percentile >= 90) return 'excellent';
	if (percentile >= 75) return 'good';
	if (percentile >= 50) return 'average';
	if (percentile >= 25) return 'below average';
	return 'poor';
}

/** Calculate basic statistics for a numeric array */
export function calculateStats(values: number[]): {
	min: number;
	max: number;
	mean: number;
	median: number;
	stdDev: number;
} {
	if (values.length === 0) {
		return { min: 0, max: 0, mean: 0, median: 0, stdDev: 0 };
	}

	const sorted = [...values].sort((a, b) => a - b);
	const min = sorted[0] ?? 0;
	const max = sorted[sorted.length - 1] ?? 0;
	const sum = sorted.reduce((a, b) => a + b, 0);
	const mean = sum / sorted.length;

	const midIndex = Math.floor(sorted.length / 2);
	const median = sorted.length % 2 === 0 ? ((sorted[midIndex - 1] ?? 0) + (sorted[midIndex] ?? 0)) / 2 : (sorted[midIndex] ?? 0);

	const squaredDiffs = sorted.map((v) => (v - mean) ** 2);
	const avgSquaredDiff = squaredDiffs.reduce((a, b) => a + b, 0) / sorted.length;
	const stdDev = Math.sqrt(avgSquaredDiff);

	return { min, max, mean, median, stdDev };
}
