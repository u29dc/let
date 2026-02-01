/**
 * Utility scoring functions
 */

import { clamp, exponentialDecay, inverseLerp, sigmoidThreshold } from './basic.js';

/** Station proximity utility function */
export function stationProximityUtility(miles: number, fullScoreThreshold = 0.5, decayRate = 1.5): number {
	if (miles <= fullScoreThreshold) return 1.0;
	return exponentialDecay(miles - fullScoreThreshold, decayRate);
}

/** Broadband availability utility function */
export function broadbandUtility(pct: number): number {
	return sigmoidThreshold(pct, 50, 0.08);
}

/** Floor area utility function */
export function floorAreaUtility(sqm: number): number {
	if (sqm < 40) return 0;
	if (sqm < 60) return inverseLerp(40, 60, sqm) * 0.6;
	if (sqm < 100) return 0.6 + inverseLerp(60, 100, sqm) * 0.3;
	return 0.9 + inverseLerp(100, 150, sqm) * 0.1;
}

/** Convert EPC band letter to numeric score */
export function epcBandToNumeric(band: string | null): number | null {
	if (!band) return null;

	const scores: Record<string, number> = {
		A: 100,
		B: 85,
		C: 70,
		D: 55,
		E: 40,
		F: 25,
		G: 10,
	};

	return scores[band.toUpperCase()] ?? null;
}

/** Get estimated monthly heating cost by EPC band */
export function getHeatingCostEstimate(band: string | null, defaultCosts: Record<string, number>): number {
	if (!band) {
		return defaultCosts['D'] ?? 100;
	}

	return defaultCosts[band.toUpperCase()] ?? defaultCosts['D'] ?? 100;
}

/** Convert IMD decile (1-10) to normalized score (0-1) */
export function imdDecileToScore(decile: number): number {
	const safe = clamp(decile, 1, 10);
	return (safe - 1) / 9;
}

/** Normalize a property type string to a canonical form */
export function normalizePropertyType(type: string | null): string | null {
	if (!type) return null;

	const normalized = type.toLowerCase().trim();

	const mappings: Record<string, string> = {
		'semi-detached': 'semi-detached',
		'semi detached': 'semi-detached',
		semidetached: 'semi-detached',
		semi: 'semi-detached',
		detached: 'detached',
		terraced: 'terraced',
		terrace: 'terraced',
		'end terrace': 'terraced',
		'end of terrace': 'terraced',
		'mid terrace': 'terraced',
		cottage: 'cottage',
		bungalow: 'bungalow',
		flat: 'flat',
		apartment: 'flat',
		maisonette: 'flat',
		studio: 'studio',
		house: 'house',
		townhouse: 'terraced',
		'town house': 'terraced',
	};

	if (mappings[normalized]) {
		return mappings[normalized];
	}

	for (const [pattern, canonical] of Object.entries(mappings)) {
		if (normalized.includes(pattern)) {
			return canonical;
		}
	}

	return normalized;
}
