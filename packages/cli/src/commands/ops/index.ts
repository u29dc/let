/**
 * Ops commands - maintenance operations
 */

import { defineCommand } from 'citty';
import { patchCommand } from './patch.js';
import { pruneCommand } from './prune.js';
import { verifyCommand } from './verify.js';

/**
 * let ops - Parent command for maintenance operations
 */
export const opsCommand = defineCommand({
	meta: {
		name: 'ops',
		description: 'Maintenance operations (patch, prune, verify)',
	},
	subCommands: {
		patch: patchCommand,
		prune: pruneCommand,
		verify: verifyCommand,
	},
});
