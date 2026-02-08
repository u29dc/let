/**
 * Assessment validation schema -- shared between assess.context output and assess.submit tool metadata.
 * Must match the Zod AssessmentSchema in @let/core/schema/assessment.ts.
 */
export const ASSESSMENT_SCHEMA = {
	type: 'object',
	required: ['maintenance', 'lightAndSpace', 'photoAnalysis', 'recommendation', 'familySuitability', 'reasoning', 'scoreAdjustment'],
	properties: {
		maintenance: { type: 'string', enum: ['excellent', 'good', 'fair', 'poor'] },
		lightAndSpace: { type: 'string' },
		photoAnalysis: { type: 'string' },
		tradeoffs: { type: 'string' },
		neighborhoodAnalysis: { type: 'string' },
		recommendation: { type: 'string', enum: ['strong-recommend', 'recommend', 'neutral', 'avoid'] },
		familySuitability: { type: 'string', enum: ['excellent', 'good', 'fair', 'poor'] },
		reasoning: { type: 'string' },
		scoreAdjustment: { type: 'number', minimum: -30, maximum: 30 },
	},
} as const;
