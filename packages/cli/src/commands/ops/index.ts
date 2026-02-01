/**
 * Ops commands - maintenance operations
 */

import { defineCommand } from 'citty';
import { enrichCommand } from './enrich.js';
import { migrateCommand } from './migrate.js';
import { pruneCommand } from './prune.js';
import { verifyCommand } from './verify.js';

/**
 * let ops - Parent command for maintenance operations
 */
export const opsCommand = defineCommand({
	meta: {
		name: 'ops',
		description: 'Maintenance operations (migrate, enrich, prune, verify)',
	},
	subCommands: {
		enrich: enrichCommand,
		migrate: migrateCommand,
		prune: pruneCommand,
		verify: verifyCommand,
	},
});
