/**
 * Assessment scoring adjustments
 */

import type { Assessment } from '@let/core/schema';
import { clamp } from '../score/index.js';

/**
 * Calculate assessed score based on algorithm score and assessment
 */
export function calculateAssessedScore(algoScore: number, assessment: Assessment): number {
	// Use explicit adjustment if provided
	if (assessment.scoreAdjustment !== undefined) {
		return clamp(algoScore + assessment.scoreAdjustment, 0, 100);
	}

	// Otherwise derive from recommendation
	const adj: Record<string, number> = {
		'strong-recommend': 10,
		recommend: 5,
		neutral: 0,
		avoid: -15,
	};
	return clamp(algoScore + (adj[assessment.recommendation] ?? 0), 0, 100);
}
