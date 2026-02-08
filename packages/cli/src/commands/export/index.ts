/**
 * `export` command group — Export listings to external formats.
 *
 * Subcommands: json, notion
 */

import { defineCommand } from 'citty';
import { exportJsonCommand } from './json.js';
import { exportNotionCommand } from './notion.js';

export const exportCommand = defineCommand({
	meta: {
		name: 'export',
		description: 'Export listings to external formats',
	},
	subCommands: {
		json: exportJsonCommand,
		notion: exportNotionCommand,
	},
});
