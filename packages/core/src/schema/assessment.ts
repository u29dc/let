/**
 * AI Assessment performed via Claude Code CLI
 *
 * This schema validates the structured input from AI analysis of property
 * photos and listing details. Assessments are submitted via:
 *   let assess <id> --json '{...}'
 *
 * The assessment captures qualitative factors that algorithms cannot measure:
 * - Maintenance quality from photos
 * - Natural light and spaciousness
 * - Photo analysis for hidden issues
 * - Trade-off evaluation for specific use cases
 */

import { z } from 'zod';

export const AssessmentSchema = z.object({
	/** Property maintenance quality from photos */
	maintenance: z.enum(['excellent', 'good', 'fair', 'poor']),
	/** Assessment of natural light and spaciousness (1-1000 chars) */
	lightAndSpace: z.string().min(1).max(1000),
	/** Photo analysis - honesty, what's shown/hidden (1-1000 chars) */
	photoAnalysis: z.string().min(1).max(1000),
	/** Trade-off evaluation for specific requirements (1-800 chars, optional) */
	tradeoffs: z.string().max(800).optional(),
	/** Neighborhood analysis from satellite imagery (1-800 chars, optional) */
	neighborhoodAnalysis: z.string().max(800).optional(),
	/** Overall recommendation based on AI analysis */
	recommendation: z.enum(['strong-recommend', 'recommend', 'neutral', 'avoid']),
	/** Family suitability assessment */
	familySuitability: z.enum(['excellent', 'good', 'fair', 'poor']),
	/** Reasoning for the recommendation (1-1000 chars) */
	reasoning: z.string().min(1).max(1000),
	/** Manual score adjustment relative to algorithm score (-30 to +30) */
	scoreAdjustment: z.number().min(-30).max(30),
});

export type Assessment = z.infer<typeof AssessmentSchema>;
