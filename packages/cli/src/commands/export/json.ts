/**
 * `export json` — Export listings database to JSON file.
 *
 * Reads SQLite and writes formatted JSON. Returns path and count.
 * Respects resolved paths from @let/core/paths.
 */

import { writeFileSync } from 'node:fs';
import { loadListingsFile } from '@let/core/db';
import { paths } from '@let/core/paths';
import { log } from '@let/core/utils/logger';
import { fail, isJsonMode, ok, rethrowCapture } from '../../envelope.js';
import { defineToolCommand } from '../../tool.js';

export const exportJsonCommand = defineToolCommand(
	{
		name: 'export.json',
		command: 'let export json',
		category: 'export',
		outputFields: ['path', 'count'],
		idempotent: true,
		rateLimit: null,
		example: 'let export json --output backup.json --json',
	},
	{
		meta: {
			name: 'json',
			description: 'Export listings database to JSON file',
		},
		args: {
			output: {
				type: 'string' as const,
				description: 'Output file path (default: derived from paths)',
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
				const dbPath = p.derived.database;
				const outputPath = args.output ?? p.derived.jsonExport;
				const data = loadListingsFile(dbPath);

				writeFileSync(outputPath, JSON.stringify(data, null, '\t'));

				const result = { path: outputPath, count: data.listings.length };

				if (jsonMode) {
					ok('export.json', result, start);
				}

				log.cli.success('JSON export saved', { path: outputPath, listings: data.listings.length });
			} catch (error) {
				rethrowCapture(error);
				const message = error instanceof Error ? error.message : String(error);
				if (jsonMode) {
					fail('export.json', 'EXPORT_ERROR', `Export failed: ${message}`, 'Check database path', start);
				}
				log.cli.error(`Export failed: ${message}`);
				process.exit(1);
			}
		},
	},
);
