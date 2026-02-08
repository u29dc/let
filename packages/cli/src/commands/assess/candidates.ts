/**
 * `assess candidates` — List unassessed listings ranked by score.
 *
 * Returns candidates ready for assessment, sorted by score descending.
 */

import { loadListingsFile } from '@let/core/db';
import { paths } from '@let/core/paths';
import { queryListings } from '@let/core/pipeline/view';
import { log } from '@let/core/utils/logger';
import { fail, isJsonMode, ok, rethrowCapture } from '../../envelope.js';
import { defineToolCommand } from '../../tool.js';

export const assessCandidatesCommand = defineToolCommand(
	{
		name: 'assess.candidates',
		command: 'let assess candidates',
		category: 'assess',
		outputFields: ['candidates', 'total', 'assessed', 'remaining'],
		idempotent: true,
		rateLimit: null,
		example: 'let assess candidates --top 10 --json',
	},
	{
		meta: {
			name: 'candidates',
			description: 'List unassessed listings ranked by score',
		},
		args: {
			top: {
				type: 'string' as const,
				description: 'Limit to top N candidates',
				default: '10',
			},
			region: {
				type: 'string' as const,
				description: 'Filter by region name',
			},
			'min-score': {
				type: 'string' as const,
				description: 'Minimum score threshold',
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
				const allListings = data.listings ?? [];
				const total = allListings.length;
				const assessed = allListings.filter((l) => l.assessment !== null).length;

				// Filter to unassessed only
				const unassessed = allListings.filter((l) => l.assessment === null && l.status === 'active');

				const top = Number.parseInt(args.top, 10);
				const minScore = args['min-score'] ? Number.parseInt(args['min-score'], 10) : undefined;

				const candidates = queryListings(unassessed, { top: Number.isNaN(top) ? 10 : top, minScore, region: args.region }, 'score', true);

				const projection = candidates.map((l) => ({
					id: l.id,
					portalId: l.portalIds.rightmove ?? null,
					address: l.address,
					score: l.scores?._overall ?? null,
					region: l.region ?? null,
					url: l.url,
				}));

				if (jsonMode) {
					ok('assess.candidates', { candidates: projection, total, assessed, remaining: total - assessed }, start);
				}

				log.cli.info(`${projection.length} candidates (${total - assessed} unassessed of ${total} total)`);
				for (const c of projection) {
					log.cli.info(`  ${c.portalId ?? c.id} | ${c.address} | score: ${c.score ?? '--'}`);
				}
			} catch (error) {
				rethrowCapture(error);
				const message = error instanceof Error ? error.message : String(error);
				if (jsonMode) {
					fail('assess.candidates', 'DB_ERROR', `Failed to load listings: ${message}`, 'Check database path', start);
				}
				log.cli.error(`Failed to load listings: ${message}`);
				process.exit(1);
			}
		},
	},
);
