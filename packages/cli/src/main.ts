/**
 * Main command definition — importable without side effects.
 *
 * Separated from index.ts so tests can import the command tree
 * and run commands in-process via citty's runCommand().
 */

import { defineCommand } from 'citty';
import { assessNewCommand } from './commands/assess/index.js';
import { configCommand } from './commands/config/index.js';
import { exportCommand } from './commands/export/index.js';
import { fetchNewCommand } from './commands/fetch/index.js';
import { healthCommand } from './commands/health/index.js';
import { opsCommand } from './commands/ops/index.js';
import { scoreCommand } from './commands/score/index.js';
import { searchCommand } from './commands/search/index.js';
import { toolsCommand } from './commands/tools/index.js';
import { viewCommand } from './commands/view/index.js';

/**
 * Root command - Property Search Agent CLI
 */
export const main = defineCommand({
	meta: {
		name: 'let',
		version: '0.0.1',
		description: 'Property Search Agent - Rightmove scraper, scorer, and viewer',
	},
	subCommands: {
		fetch: fetchNewCommand,
		assess: assessNewCommand,
		config: configCommand,
		view: viewCommand,
		export: exportCommand,
		ops: opsCommand,
		score: scoreCommand,
		search: searchCommand,
		tools: toolsCommand,
		health: healthCommand,
	},
});
