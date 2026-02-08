/**
 * `view list` — Ranked listings table with filters.
 *
 * Supports --top, --min-score, --sort, --region, --type, --asc filters.
 * JSON mode returns projection array; text mode uses table formatter.
 */

import { loadListingsFile } from '@let/core/db';
import { paths } from '@let/core/paths';
import { formatTableRow, queryListings, type SortField } from '@let/core/pipeline/view';
import { log } from '@let/core/utils/logger';
import { fail, isJsonMode, ok, rethrowCapture } from '../../envelope.js';
import { defineToolCommand } from '../../tool.js';

const VALID_SORT_FIELDS: SortField[] = ['score', 'price', 'bedrooms', 'date'];

function parseSortField(value: string): SortField {
	if (VALID_SORT_FIELDS.includes(value as SortField)) return value as SortField;
	return 'score';
}

function loadListings(dbPath: string) {
	const data = loadListingsFile(dbPath);
	return data.listings ?? [];
}

export const viewListCommand = defineToolCommand(
	{
		name: 'view.list',
		command: 'let view list',
		category: 'view',
		outputSchema: {
			listings: { type: 'array', items: 'TableRow', description: 'Listings as table rows: id, address, price, priceDisplay, bedrooms, score, assessedScore, scoreChange, station, region, url' },
			total: { type: 'number', description: 'Total listings in database before filtering' },
			filtered: { type: 'number', description: 'Count after applying filters' },
		},
		idempotent: true,
		rateLimit: null,
		example: 'let view list --top 10 --region Sheffield --json',
	},
	{
		meta: {
			name: 'list',
			description: 'Ranked listings table with filters',
		},
		args: {
			top: {
				type: 'string' as const,
				description: 'Limit to top N by score',
				default: '20',
			},
			'min-score': {
				type: 'string' as const,
				description: 'Minimum score threshold (0-100)',
			},
			sort: {
				type: 'string' as const,
				description: 'Sort by: score, price, bedrooms, date',
				default: 'score',
			},
			asc: {
				type: 'boolean' as const,
				description: 'Ascending order (default: descending)',
				default: false,
			},
			region: {
				type: 'string' as const,
				description: 'Filter by region name',
			},
			type: {
				type: 'string' as const,
				description: 'Filter by property type (comma-separated)',
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
				const listings = loadListings(dbPath);

				if (listings.length === 0) {
					if (jsonMode) {
						ok('view.list', { listings: [], total: 0, filtered: 0 }, start);
					}
					log.cli.warn('No listings found. Run "let fetch" first.');
					return;
				}

				const top = Number.parseInt(args.top, 10);
				const minScore = args['min-score'] ? Number.parseInt(args['min-score'], 10) : undefined;
				const sortField = parseSortField(args.sort);
				const desc = !args.asc;

				const filtered = queryListings(listings, { top: Number.isNaN(top) ? 20 : top, minScore, region: args.region, type: args.type }, sortField, desc);

				if (jsonMode) {
					const projection = filtered.map((l) => formatTableRow(l));
					ok('view.list', { listings: projection, total: listings.length, filtered: projection.length }, start);
				}

				// Text output (import renderTable dynamically to avoid circular deps)
				log.cli.info(`Showing ${filtered.length} of ${listings.length} listings`);
				const { renderTable } = await import('./index.js');
				renderTable(filtered);
			} catch (error) {
				rethrowCapture(error);
				const message = error instanceof Error ? error.message : String(error);
				if (jsonMode) {
					fail('view.list', 'DB_ERROR', `Failed to load listings: ${message}`, 'Check database path', start);
				}
				log.cli.error(`Failed to load listings: ${message}`);
				process.exit(1);
			}
		},
	},
);
