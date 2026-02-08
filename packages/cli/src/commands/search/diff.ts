/**
 * `search diff <ids...>` — Compare discovered IDs against known listings.
 *
 * Accepts comma-separated portal IDs (or piped from discover).
 * Returns new vs known partitions. Deterministic.
 */

import { loadListingsFile } from '@let/core/db';
import { paths } from '@let/core/paths';
import { log } from '@let/core/utils/logger';
import { fail, isJsonMode, ok, rethrowCapture } from '../../envelope.js';
import { defineToolCommand } from '../../tool.js';

function parseInputIds(raw: string): string[] {
	return raw
		.split(',')
		.map((id: string) => id.trim())
		.filter(Boolean);
}

function buildKnownIdSet(dbPath: string): Set<string> {
	try {
		const data = loadListingsFile(dbPath);
		const listings = data.listings ?? [];
		const ids = new Set<string>();
		for (const l of listings) {
			if (l.portalIds.rightmove) ids.add(l.portalIds.rightmove);
			if (l.portalIds.zoopla) ids.add(l.portalIds.zoopla);
			if (l.portalIds.onthemarket) ids.add(l.portalIds.onthemarket);
		}
		return ids;
	} catch {
		return new Set();
	}
}

function partitionIds(inputIds: string[], knownIds: Set<string>) {
	const newIds: string[] = [];
	const known: string[] = [];
	for (const id of inputIds) {
		if (knownIds.has(id)) {
			known.push(id);
		} else {
			newIds.push(id);
		}
	}
	return { newIds, known };
}

export const searchDiffCommand = defineToolCommand(
	{
		name: 'search.diff',
		command: 'let search diff',
		category: 'search',
		outputFields: ['new', 'known', 'total'],
		idempotent: true,
		rateLimit: null,
		example: 'let search diff 170448131,170448132 --json',
	},
	{
		meta: {
			name: 'diff',
			description: 'Compare IDs against known listings',
		},
		args: {
			ids: {
				type: 'positional' as const,
				description: 'Comma-separated portal IDs to check',
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

			try {
				const inputIds = parseInputIds(args.ids);

				if (inputIds.length === 0) {
					if (jsonMode) {
						fail('search.diff', 'VALIDATION_ERROR', 'No IDs provided', 'Provide comma-separated portal IDs', start);
					}
					log.cli.error('No IDs provided');
					process.exit(1);
				}

				const knownIds = buildKnownIdSet(p.derived.database);
				const { newIds, known } = partitionIds(inputIds, knownIds);

				if (jsonMode) {
					ok('search.diff', { new: newIds, known, total: inputIds.length }, start);
				}

				log.cli.info(`${inputIds.length} IDs checked: ${newIds.length} new, ${known.length} known`);
				if (newIds.length > 0) {
					log.cli.info(`New: ${newIds.join(', ')}`);
				}
			} catch (error) {
				rethrowCapture(error);
				const message = error instanceof Error ? error.message : String(error);
				if (jsonMode) {
					fail('search.diff', 'DB_ERROR', `Failed to check listings: ${message}`, 'Check database path', start);
				}
				log.cli.error(`Failed to check listings: ${message}`);
				process.exit(1);
			}
		},
	},
);
