/**
 * `config show [section]` — Show parsed configuration.
 *
 * Returns the full parsed config or a specific section (search/fetch/scoring).
 * Uses resolved config path from paths().
 */

import { loadConfig, resetConfigCache } from '@let/core/config';
import type { DerivedPaths } from '@let/core/paths';
import { paths } from '@let/core/paths';
import { log } from '@let/core/utils/logger';
import { fail, isJsonMode, ok, rethrowCapture } from '../../envelope.js';
import { defineToolCommand } from '../../tool.js';

const VALID_SECTIONS = ['search', 'fetch', 'scoring'] as const;

function handleNotFound(jsonMode: boolean, configPath: string, derived: DerivedPaths, start: number): never {
	if (jsonMode) {
		fail('config.show', 'NO_CONFIG', `Config file not found: ${configPath}`, `cp ${derived.templateFile} ${configPath}`, start);
	}
	log.cli.error(`Config not found: ${configPath}`);
	process.exit(1);
}

function handleInvalidConfig(jsonMode: boolean, message: string, start: number): never {
	if (jsonMode) {
		fail('config.show', 'INVALID_CONFIG', `Config validation failed: ${message}`, 'Check config file against template', start);
	}
	log.cli.error(`Config error: ${message}`);
	process.exit(1);
}

export const configShowCommand = defineToolCommand(
	{
		name: 'config.show',
		command: 'let config show',
		category: 'config',
		outputFields: ['path', 'config'],
		idempotent: true,
		rateLimit: null,
		example: 'let config show --json',
	},
	{
		meta: {
			name: 'show',
			description: 'Show parsed configuration',
		},
		args: {
			section: {
				type: 'positional' as const,
				description: 'Section to show (search, fetch, scoring)',
				required: false,
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
			const configPath = p.derived.configFile;

			resetConfigCache();

			try {
				const config = await loadConfig(configPath);
				const section = args.section;

				if (section && !VALID_SECTIONS.includes(section as (typeof VALID_SECTIONS)[number])) {
					if (jsonMode) {
						fail('config.show', 'VALIDATION_ERROR', `Unknown section: ${section}`, `Valid sections: ${VALID_SECTIONS.join(', ')}`, start);
					}
					log.cli.error(`Unknown section: ${section}. Valid: ${VALID_SECTIONS.join(', ')}`);
					process.exit(1);
				}

				const data = {
					path: configPath,
					config: section ? { [section]: config[section as keyof typeof config] } : config,
				};

				if (jsonMode) {
					ok('config.show', data, start);
				}

				log.cli.info(`Config: ${configPath}`);
				log.cli.info(JSON.stringify(data.config, null, 2));
			} catch (error) {
				rethrowCapture(error);
				const message = error instanceof Error ? error.message : String(error);
				const isNotFound = message.includes('No such file') || message.includes('ENOENT');

				if (isNotFound) {
					handleNotFound(jsonMode, configPath, p.derived, start);
				}
				handleInvalidConfig(jsonMode, message, start);
			}
		},
	},
);
