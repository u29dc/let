/**
 * `search resolve <location>` — Location lookup via TypeAhead API.
 *
 * Resolves a city/area name to Rightmove location identifiers.
 * Rate-limit safe (single API call per location).
 */

import { lookupLocation } from '@let/core/pipeline/fetch';
import { log } from '@let/core/utils/logger';
import { fail, isJsonMode, ok } from '../../envelope.js';
import { defineToolCommand } from '../../tool.js';

export const searchResolveCommand = defineToolCommand(
	{
		name: 'search.resolve',
		command: 'let search resolve',
		category: 'search',
		outputFields: ['query', 'locations'],
		idempotent: true,
		rateLimit: '1 req per lookup',
		example: 'let search resolve York --json',
	},
	{
		meta: {
			name: 'resolve',
			description: 'Resolve location name to search identifier',
		},
		args: {
			location: {
				type: 'positional' as const,
				description: 'City or area name to resolve',
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

			try {
				const result = await lookupLocation(args.location);

				if (!result.success) {
					if (jsonMode) {
						fail('search.resolve', 'LOOKUP_ERROR', `Location lookup failed: ${result.error}`, 'Check location name spelling', start);
					}
					log.cli.error(`Location lookup failed: ${result.error}`);
					process.exit(1);
				}

				const locations = result.locations.map((loc) => ({
					displayName: loc.displayName,
					locationIdentifier: loc.locationIdentifier,
					normalizedSearchTerm: loc.normalizedSearchTerm,
				}));

				if (jsonMode) {
					ok('search.resolve', { query: args.location, locations }, start);
				}

				log.cli.info(`Resolved "${args.location}" → ${locations.length} result(s)`);
				for (const loc of locations) {
					log.cli.info(`  ${loc.displayName} → ${loc.locationIdentifier}`);
				}
			} catch (error) {
				const message = error instanceof Error ? error.message : String(error);
				if (jsonMode) {
					fail('search.resolve', 'NETWORK_ERROR', `Location lookup failed: ${message}`, 'Check network connectivity', start);
				}
				log.cli.error(`Location lookup failed: ${message}`);
				process.exit(1);
			}
		},
	},
);
