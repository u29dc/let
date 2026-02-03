/**
 * Assessment scoring adjustments
 */

import type { Assessment } from '@let/core/schema';
import { clamp } from '../score/index.js';

/**
 * Calculate assessed score based on algorithm score and assessment
 */
export function calculateAssessedScore(algoScore: number, assessment: Assessment): number {
	return clamp(algoScore + assessment.scoreAdjustment, 0, 100);
}
