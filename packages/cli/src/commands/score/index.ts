/**
 * `score` command group — Score analysis.
 *
 * Subcommands: explain, compute
 */

import { defineCommand } from 'citty';
import { scoreComputeCommand } from './compute.js';
import { scoreExplainCommand } from './explain.js';

export const scoreCommand = defineCommand({
	meta: {
		name: 'score',
		description: 'Score analysis',
	},
	subCommands: {
		explain: scoreExplainCommand,
		compute: scoreComputeCommand,
	},
});
