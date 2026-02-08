/**
 * `score` command group — Score analysis.
 *
 * Subcommands: explain (compute added later in Stage 4)
 */

import { defineCommand } from 'citty';
import { scoreExplainCommand } from './explain.js';

export const scoreCommand = defineCommand({
	meta: {
		name: 'score',
		description: 'Score analysis',
	},
	subCommands: {
		explain: scoreExplainCommand,
	},
});
