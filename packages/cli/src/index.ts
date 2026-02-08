#!/usr/bin/env bun
/**
 * CLI entry point for Property Search Agent
 *
 * Commands:
 * - let fetch     Fetch listings by portal ID
 * - let assess    View or submit AI assessment
 * - let view      Display and analytics (list, detail, stats, regions)
 * - let export    Export to external services (notion, json)
 * - let ops       Maintenance operations (prune, verify)
 *
 * Usage: bun run let <command> [options]
 *
 * Commands are imported statically for clarity.
 */

import { runMain } from 'citty';
import { setupSignalHandlers } from './commands/shared-read.js';
import { main } from './main.js';

// Setup graceful shutdown (minimal import from shared-read)
setupSignalHandlers();

runMain(main);
