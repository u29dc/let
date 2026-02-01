#!/usr/bin/env bun
/**
 * CLI entry point for Property Search Agent
 *
 * Commands:
 * - let fetch     Data acquisition (single ID or batch from config)
 * - let assess    View or submit AI assessment
 * - let view      Display and analytics (list, detail, stats, regions)
 * - let output    Export to external services (notion, json)
 * - let ops       Maintenance operations (prune, verify)
 *
 * Usage: bun run let <command> [options]
 *
 * Commands are imported statically for clarity.
 */

import { defineCommand, runMain } from 'citty';
import { assessCommand } from './commands/assess.js';
import { fetchCommand } from './commands/fetch.js';
import { helpCommand } from './commands/help.js';
import { opsCommand } from './commands/ops/index.js';
import { outputCommand } from './commands/output/index.js';
import { setupSignalHandlers } from './commands/shared-read.js';
import { viewCommand } from './commands/view/index.js';

// Setup graceful shutdown (minimal import from shared-read)
setupSignalHandlers();

/**
 * Root command - Property Search Agent CLI
 */
const main = defineCommand({
	meta: {
		name: 'let',
		version: '0.0.1',
		description: 'Property Search Agent - Rightmove scraper, scorer, and viewer',
	},
	subCommands: {
		fetch: fetchCommand,
		assess: assessCommand,
		view: viewCommand,
		output: outputCommand,
		ops: opsCommand,
		help: helpCommand,
	},
});

runMain(main);
