/**
 * `score explain <id>` — Score breakdown for one listing.
 *
 * Returns composite scores, factors, and penalties without judgment.
 */

import { loadListingsFile } from '@let/core/db';
import { paths } from '@let/core/paths';
import { findListingById } from '@let/core/pipeline/view';
import { log } from '@let/core/utils/logger';
import { fail, isJsonMode, ok, rethrowCapture } from '../../envelope.js';
import { defineToolCommand } from '../../tool.js';

function buildBreakdown(listing: { id: string; scores: NonNullable<import('@let/core/schema').Listing['scores']>; assessedScore: number | null }) {
	const s = listing.scores;
	return {
		id: listing.id,
		overall: s._overall,
		assessedScore: listing.assessedScore,
		confidence: s.confidence,
		composites: {
			affordability: {
				score: s.affordability,
				factors: { trueMonthlyCost: s.factors.trueMonthlyCost, trueCostPercentile: s.factors.trueCostPercentile, epcBand: s.factors.epcBand, epcNumeric: s.factors.epcNumeric },
			},
			location: {
				score: s.location,
				factors: {
					stationMiles: s.factors.stationMiles,
					stationPercentile: s.factors.stationPercentile,
					gigabitPct: s.factors.gigabitPct,
					regionName: s.factors.regionName,
					priorityScore: s.factors.priorityScore,
					imdDecile: s.factors.imdDecile,
					crimeRatePer1k: s.factors.crimeRatePer1k,
				},
			},
			liveability: {
				score: s.liveability,
				factors: { gardenType: s.factors.gardenType, heatingType: s.factors.heatingType, petPolicy: s.factors.petPolicy, propertyType: s.factors.propertyType, bedrooms: s.factors.bedrooms },
			},
		},
		penalties: s.penalties,
	};
}

export const scoreExplainCommand = defineToolCommand(
	{
		name: 'score.explain',
		command: 'let score explain',
		category: 'score',
		outputFields: ['id', 'overall', 'assessedScore', 'confidence', 'composites', 'penalties'],
		idempotent: true,
		rateLimit: null,
		example: 'let score explain 170448131 --json',
	},
	{
		meta: {
			name: 'explain',
			description: 'Show score breakdown for a listing',
		},
		args: {
			id: {
				type: 'positional' as const,
				description: 'Listing UUID or portal ID',
				required: true,
			},
			json: {
				type: 'boolean' as const,
				description: 'Output as JSON envelope',
				default: false,
			},
		},
		async run({ args }) {
			const start = performance.now();
			const jsonMode = isJsonMode();
			const p = paths();
			const dbPath = p.derived.database;

			try {
				const data = loadListingsFile(dbPath);
				const listings = data.listings ?? [];
				const listing = findListingById(listings, args.id);

				if (!listing) {
					if (jsonMode) {
						fail('score.explain', 'NOT_FOUND', `Listing not found: ${args.id}`, 'Check ID with "let view list"', start);
					}
					log.cli.error(`Listing not found: ${args.id}`);
					process.exit(1);
				}

				if (!listing.scores) {
					if (jsonMode) {
						fail('score.explain', 'NOT_SCORED', `Listing has no scores: ${args.id}`, 'Run "let score compute" first', start);
					}
					log.cli.error(`Listing has no scores: ${args.id}`);
					process.exit(1);
				}

				const breakdown = buildBreakdown({ id: listing.id, scores: listing.scores, assessedScore: listing.assessedScore });

				if (jsonMode) {
					ok('score.explain', breakdown, start);
				}

				// Text output
				log.cli.info(`Score breakdown for ${args.id}`);
				log.cli.info(`  Overall: ${breakdown.overall}`);
				log.cli.info(`  Assessed: ${breakdown.assessedScore ?? '--'}`);
				log.cli.info(`  Confidence: ${(breakdown.confidence * 100).toFixed(0)}%`);
				log.cli.info('  Composites:');
				log.cli.info(`    Affordability: ${breakdown.composites.affordability.score}`);
				log.cli.info(`    Location: ${breakdown.composites.location.score}`);
				log.cli.info(`    Liveability: ${breakdown.composites.liveability.score}`);
				log.cli.info('  Penalties:');
				log.cli.info(`    EPC: ${breakdown.penalties.epc}`);
				log.cli.info(`    Garden: ${breakdown.penalties.garden}`);
				log.cli.info(`    Pets: ${breakdown.penalties.pets}`);
				log.cli.info(`    Combined: ${breakdown.penalties.combined}`);
			} catch (error) {
				rethrowCapture(error);
				const message = error instanceof Error ? error.message : String(error);
				if (jsonMode) {
					fail('score.explain', 'DB_ERROR', `Failed to load listings: ${message}`, 'Check database path', start);
				}
				log.cli.error(`Failed to load listings: ${message}`);
				process.exit(1);
			}
		},
	},
);
