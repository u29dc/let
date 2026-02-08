/**
 * `assess` command group — Assessment workflow.
 *
 * Subcommands: candidates, context, submit
 */

import { defineCommand } from 'citty';
import { assessCandidatesCommand } from './candidates.js';
import { assessContextCommand } from './context.js';
import { assessSubmitCommand } from './submit.js';

export const assessNewCommand = defineCommand({
	meta: {
		name: 'assess',
		description: 'Assessment workflow',
	},
	subCommands: {
		candidates: assessCandidatesCommand,
		context: assessContextCommand,
		submit: assessSubmitCommand,
	},
});
