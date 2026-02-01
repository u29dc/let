/**
 * Basic math helpers for scoring
 */

/** Clamp a value between min and max */
export function clamp(value: number, min: number, max: number): number {
	return Math.max(min, Math.min(max, value));
}

/** Linear interpolation between two values */
export function lerp(a: number, b: number, t: number): number {
	return a + (b - a) * clamp(t, 0, 1);
}

/** Inverse linear interpolation */
export function inverseLerp(a: number, b: number, value: number): number {
	if (a === b) return 0;
	return clamp((value - a) / (b - a), 0, 1);
}

/** Standard sigmoid function */
export function sigmoid(x: number, steepness = 1): number {
	return 1 / (1 + Math.exp(-steepness * x));
}

/** Shifted sigmoid function centered at a threshold */
export function sigmoidThreshold(value: number, threshold: number, steepness = 0.1): number {
	return sigmoid((value - threshold) * steepness);
}

/** Exponential decay function */
export function exponentialDecay(x: number, rate = 1): number {
	return Math.exp(-rate * Math.max(0, x));
}

/** Round to specified decimal places */
export function roundTo(value: number, decimals: number): number {
	const factor = 10 ** decimals;
	return Math.round(value * factor) / factor;
}
