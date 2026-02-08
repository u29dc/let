/**
 * `view detail <id>` — Full listing details.
 *
 * Retrieves by UUID or portal ID. JSON mode returns full listing object.
 */

import { loadListingsFile } from '@let/core/db';
import { paths } from '@let/core/paths';
import { findListingById } from '@let/core/pipeline/view';
import { log } from '@let/core/utils/logger';
import { fail, isJsonMode, ok, rethrowCapture } from '../../envelope.js';
import { defineToolCommand } from '../../tool.js';

export const viewDetailCommand = defineToolCommand(
	{
		name: 'view.detail',
		command: 'let view detail',
		category: 'view',
		outputFields: ['listing'],
		idempotent: true,
		rateLimit: null,
		example: 'let view detail 170448131 --json',
	},
	{
		meta: {
			name: 'detail',
			description: 'Full listing details by ID',
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

				if (listings.length === 0) {
					if (jsonMode) {
						fail('view.detail', 'NO_DATA', 'No listings in database', 'Run "let fetch" first', start);
					}
					log.cli.warn('No listings found. Run "let fetch" first.');
					process.exit(1);
				}

				const listing = findListingById(listings, args.id);

				if (!listing) {
					if (jsonMode) {
						fail('view.detail', 'NOT_FOUND', `Listing not found: ${args.id}`, 'Check ID with "let view list"', start);
					}
					log.cli.error(`Listing not found: ${args.id}`);
					process.exit(1);
				}

				if (jsonMode) {
					ok('view.detail', { listing }, start);
				}

				const { renderDetail } = await import('./index.js');
				renderDetail(listing);
			} catch (error) {
				rethrowCapture(error);
				const message = error instanceof Error ? error.message : String(error);
				if (jsonMode) {
					fail('view.detail', 'DB_ERROR', `Failed to load listings: ${message}`, 'Check database path', start);
				}
				log.cli.error(`Failed to load listings: ${message}`);
				process.exit(1);
			}
		},
	},
);
