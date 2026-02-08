/**
 * `assess` command group — Assessment workflow (read-only first).
 *
 * Subcommands: candidates, context (submit added later in Stage 4)
 *
 * Note: The legacy `assess` command (assess.ts) remains for backward compat.
 * This module provides the new agent-native subcommands.
 */

import { defineCommand } from 'citty';
import { assessCandidatesCommand } from './candidates.js';
import { assessContextCommand } from './context.js';

export const assessNewCommand = defineCommand({
	meta: {
		name: 'assess',
		description: 'Assessment workflow',
	},
	subCommands: {
		candidates: assessCandidatesCommand,
		context: assessContextCommand,
	},
});
