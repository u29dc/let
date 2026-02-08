/**
 * `config` command group — Configuration management.
 *
 * Subcommands: show, validate
 */

import { defineCommand } from 'citty';
import { configShowCommand } from './show.js';
import { configValidateCommand } from './validate.js';

export const configCommand = defineCommand({
	meta: {
		name: 'config',
		description: 'Configuration management',
	},
	subCommands: {
		show: configShowCommand,
		validate: configValidateCommand,
	},
});
