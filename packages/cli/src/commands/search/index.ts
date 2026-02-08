/**
 * `search` command group — Location resolution and listing discovery.
 *
 * Subcommands: resolve, discover, diff
 */

import { defineCommand } from 'citty';
import { searchDiffCommand } from './diff.js';
import { searchDiscoverCommand } from './discover.js';
import { searchResolveCommand } from './resolve.js';

export const searchCommand = defineCommand({
	meta: {
		name: 'search',
		description: 'Location resolution and listing discovery',
	},
	subCommands: {
		resolve: searchResolveCommand,
		discover: searchDiscoverCommand,
		diff: searchDiffCommand,
	},
});
