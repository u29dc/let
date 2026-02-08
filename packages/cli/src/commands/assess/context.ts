/**
 * `assess context <id>` — Assessment context bundle.
 *
 * Returns listing + score breakdown + assessment schema + media paths + links.
 * Used by AI agents to prepare assessment submissions.
 */

import { existsSync } from 'node:fs';
import { resolve } from 'node:path';
import { loadListingsFile } from '@let/core/db';
import { paths } from '@let/core/paths';
import { findListingById } from '@let/core/pipeline/view';
import { log } from '@let/core/utils/logger';
import { fail, isJsonMode, ok } from '../../envelope.js';
import { defineToolCommand } from '../../tool.js';

function resolveMediaPaths(listing: import('@let/core/schema').Listing, cacheDir: string) {
	const id = listing.portalIds.rightmove ?? listing.id;
	const entryDir = resolve(cacheDir, id);

	const images: string[] = [];
	for (const img of listing.images) {
		if (img.local) {
			const abs = resolve(cacheDir, img.local);
			if (existsSync(abs)) images.push(abs);
		}
	}

	return {
		images,
		floorplan: listing.floorplan.local ? resolve(cacheDir, listing.floorplan.local) : null,
		satellite: listing.mapViews?.satellite?.local ? resolve(cacheDir, listing.mapViews.satellite.local) : null,
		street: listing.mapViews?.street?.local ? resolve(cacheDir, listing.mapViews.street.local) : null,
		cacheDir: existsSync(entryDir) ? entryDir : null,
	};
}

function buildScoreBreakdown(listing: import('@let/core/schema').Listing) {
	if (!listing.scores) return null;
	const s = listing.scores;
	return {
		overall: s._overall,
		assessedScore: listing.assessedScore,
		confidence: s.confidence,
		affordability: s.affordability,
		location: s.location,
		liveability: s.liveability,
		factors: s.factors,
		penalties: s.penalties,
	};
}

const ASSESSMENT_SCHEMA = {
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
};

export const assessContextCommand = defineToolCommand(
	{
		name: 'assess.context',
		command: 'let assess context',
		category: 'assess',
		outputFields: ['listing', 'scoreBreakdown', 'assessmentSchema', 'media', 'links', 'description', 'notes'],
		idempotent: true,
		rateLimit: null,
		example: 'let assess context 170448131 --json',
	},
	{
		meta: {
			name: 'context',
			description: 'Assessment context bundle for a listing',
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
						fail('assess.context', 'NOT_FOUND', `Listing not found: ${args.id}`, 'Check ID with "let view list"', start);
					}
					log.cli.error(`Listing not found: ${args.id}`);
					process.exit(1);
				}

				const scoreBreakdown = buildScoreBreakdown(listing);
				const media = resolveMediaPaths(listing, p.resolved.cache);
				const links = {
					rightmove: listing.url,
					googleMaps: listing.googleMapsUrl,
					streetView: listing.googleMapsStreetViewUrl,
					epcSearch: listing.epcSearchUrl ?? null,
				};

				const context = {
					listing,
					scoreBreakdown,
					assessmentSchema: ASSESSMENT_SCHEMA,
					media,
					links,
					description: listing.description,
					notes: listing.notes,
				};

				if (jsonMode) {
					ok('assess.context', context, start);
				}

				log.cli.info(`Context for ${args.id}:`);
				log.cli.info(`  Address: ${listing.address}`);
				log.cli.info(`  Score: ${scoreBreakdown?.overall ?? '--'}`);
				log.cli.info(`  Images: ${media.images.length}`);
				log.cli.info(`  Links: ${Object.values(links).filter(Boolean).length}`);
				log.cli.info(`  Notes: ${listing.notes.length}`);
				log.cli.info(`  Description: ${listing.description.slice(0, 100)}...`);
			} catch (error) {
				const message = error instanceof Error ? error.message : String(error);
				if (jsonMode) {
					fail('assess.context', 'DB_ERROR', `Failed to load listings: ${message}`, 'Check database path', start);
				}
				log.cli.error(`Failed to load listings: ${message}`);
				process.exit(1);
			}
		},
	},
);
