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
import { assessNewCommand } from './commands/assess/index.js';
import { configCommand } from './commands/config/index.js';
import { fetchCommand } from './commands/fetch.js';
import { healthCommand } from './commands/health/index.js';
import { helpCommand } from './commands/help.js';
import { opsCommand } from './commands/ops/index.js';
import { outputCommand } from './commands/output/index.js';
import { scoreCommand } from './commands/score/index.js';
import { searchCommand } from './commands/search/index.js';
import { setupSignalHandlers } from './commands/shared-read.js';
import { toolsCommand } from './commands/tools/index.js';
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
		assess: assessNewCommand,
		config: configCommand,
		view: viewCommand,
		output: outputCommand,
		ops: opsCommand,
		score: scoreCommand,
		search: searchCommand,
		help: helpCommand,
		tools: toolsCommand,
		health: healthCommand,
	},
});

runMain(main);
