/**
 * Normalize assessment fields for consistency
 */

import type { Assessment } from '@let/core/schema';

/** Lowercase string fields in assessment for consistency */
export function normalizeAssessment(assessment: Assessment): Assessment {
	return {
		...assessment,
		lightAndSpace: assessment.lightAndSpace.toLowerCase(),
		photoAnalysis: assessment.photoAnalysis.toLowerCase(),
		reasoning: assessment.reasoning.toLowerCase(),
		tradeoffs: assessment.tradeoffs?.toLowerCase(),
		neighborhoodAnalysis: assessment.neighborhoodAnalysis?.toLowerCase(),
	};
}
