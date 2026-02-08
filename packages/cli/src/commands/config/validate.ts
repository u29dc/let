/**
 * `config validate` — Validate config file.
 *
 * Returns structured validation results with field-specific errors.
 */

import { existsSync } from 'node:fs';
import { ConfigSchema, resetConfigCache } from '@let/core/config';
import { paths } from '@let/core/paths';
import { log } from '@let/core/utils/logger';
import { emitRaw, fail, isJsonMode, ok, rethrowCapture } from '../../envelope.js';
import { defineToolCommand } from '../../tool.js';

export const configValidateCommand = defineToolCommand(
	{
		name: 'config.validate',
		command: 'let config validate',
		category: 'config',
		outputFields: ['valid', 'path', 'errors'],
		idempotent: true,
		rateLimit: null,
		example: 'let config validate --json',
	},
	{
		meta: {
			name: 'validate',
			description: 'Validate config file',
		},
		args: {
			json: {
				type: 'boolean' as const,
				description: 'Output as JSON envelope',
				default: false,
			},
		},
		async run() {
			const start = performance.now();
			const jsonMode = isJsonMode();
			const p = paths();
			const configPath = p.derived.configFile;

			resetConfigCache();

			if (!existsSync(configPath)) {
				if (jsonMode) {
					fail('config.validate', 'NO_CONFIG', `Config file not found: ${configPath}`, `cp ${p.derived.templateFile} ${configPath}`, start);
				}
				log.cli.error(`Config not found: ${configPath}`);
				process.exit(1);
			}

			try {
				const file = Bun.file(configPath);
				const text = await file.text();
				const raw = Bun.TOML.parse(text);
				const result = ConfigSchema.safeParse(raw);

				if (result.success) {
					const data = { valid: true, path: configPath, errors: [] };
					if (jsonMode) {
						ok('config.validate', data, start);
					}
					log.cli.info(`Config valid: ${configPath}`);
					return;
				}

				const errors = result.error.issues.map((issue) => ({
					path: issue.path.join('.'),
					message: issue.message,
				}));

				if (jsonMode) {
					const data = { valid: false, path: configPath, errors };
					const elapsed = Math.round(performance.now() - start);
					const envelope = { ok: true, data, meta: { tool: 'config.validate', elapsed } };
					emitRaw(JSON.stringify(envelope), 1);
				}

				log.cli.error(`Config invalid: ${configPath}`);
				for (const err of errors) {
					log.cli.error(`  ${err.path}: ${err.message}`);
				}
				process.exit(1);
			} catch (error) {
				rethrowCapture(error);
				const message = error instanceof Error ? error.message : String(error);
				if (jsonMode) {
					fail('config.validate', 'INVALID_CONFIG', `Failed to parse config: ${message}`, 'Check TOML syntax', start);
				}
				log.cli.error(`Parse error: ${message}`);
				process.exit(1);
			}
		},
	},
);
