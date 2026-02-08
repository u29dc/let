/**
 * Ops commands - maintenance operations
 */

import { defineCommand } from 'citty';
import { pruneCommand } from './prune.js';
import { verifyCommand } from './verify.js';

/**
 * let ops - Parent command for maintenance operations
 */
export const opsCommand = defineCommand({
	meta: {
		name: 'ops',
		description: 'Maintenance operations (prune, verify)',
	},
	subCommands: {
		prune: pruneCommand,
		verify: verifyCommand,
	},
});
