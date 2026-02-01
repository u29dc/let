/**
 * Factor normalization
 */

import { calculatePercentile } from '../math/percentiles.js';
import { epcBandToNumeric, getHeatingCostEstimate } from '../math/utilities.js';
import { matchRegionName } from '../regions.js';
import type { NormalizedFactors, PercentileContext, RawFactors, ScoringConfig } from '../types.js';

/** Normalize factors with percentile context and config */
export function normalizeFactors(raw: RawFactors, percentiles: PercentileContext, config: ScoringConfig): NormalizedFactors {
	const heatingCost = getHeatingCostEstimate(raw.epcBand, config.affordability.heatingCosts);
	const trueMonthlyCost = raw.monthlyRent + heatingCost;

	const pricePercentile = calculatePercentile(raw.monthlyRent, percentiles.prices, true);
	const trueCostPercentile = calculatePercentile(trueMonthlyCost, percentiles.trueCosts, true);

	const floorAreaPercentile = raw.floorAreaSqm !== null ? calculatePercentile(raw.floorAreaSqm, percentiles.floorAreas, false) : null;

	const stationPercentile = raw.stationMiles !== null ? calculatePercentile(raw.stationMiles, percentiles.stationDistances, true) : null;

	const crimeRatePercentile = raw.crimeRatePer1k !== null ? calculatePercentile(raw.crimeRatePer1k, percentiles.crimeRates, true) : null;

	const regionKeys = Object.keys(config.regionPriority);
	const matchedRegion = raw.regionName ? matchRegionName(raw.regionName, regionKeys) : null;
	const priorityScore = matchedRegion ? (config.regionPriority[matchedRegion] ?? null) : null;

	return {
		...raw,
		regionName: matchedRegion ?? raw.regionName,
		pricePercentile,
		trueCostPercentile,
		floorAreaPercentile,
		stationPercentile,
		trueMonthlyCost,
		epcNumeric: epcBandToNumeric(raw.epcBand),
		priorityScore,
		crimeRatePercentile,
	};
}
