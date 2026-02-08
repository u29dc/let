/**
 * `tools [name]` — Capability discovery from tool registry.
 *
 * Returns the full tool catalog or detail for a single tool.
 * Generated from toolRegistry[] (single source of truth).
 */

import { type ArgsDef, defineCommand } from 'citty';
import { fail, isJsonMode, ok } from '../../envelope.js';
import type { ToolMeta } from '../../tool.js';
import { toolRegistry } from '../../tool.js';

const GLOBAL_FLAGS = [
	{ name: '--json', description: 'Output as JSON envelope' },
	{ name: '--data-dir', description: 'Override data directory' },
	{ name: '--cache-dir', description: 'Override cache directory' },
	{ name: '--config-dir', description: 'Override config directory' },
	{ name: '--sources-dir', description: 'Override sources directory' },
];

function getSortedRegistry(): ToolMeta[] {
	return [...toolRegistry].sort((a, b) => {
		const catCmp = a.category.localeCompare(b.category);
		if (catCmp !== 0) return catCmp;
		return a.name.localeCompare(b.name);
	});
}

function showToolDetail(tool: ToolMeta): void {
	process.stderr.write(`${tool.name} — ${tool.description}\n`);
	process.stderr.write(`  Command: ${tool.command}\n`);
	process.stderr.write(`  Category: ${tool.category}\n`);
	process.stderr.write(`  Idempotent: ${tool.idempotent}\n`);
	if (tool.rateLimit) process.stderr.write(`  Rate limit: ${tool.rateLimit}\n`);
	process.stderr.write(`  Example: ${tool.example}\n`);
	if (tool.parameters.length > 0) {
		process.stderr.write('  Parameters:\n');
		for (const p of tool.parameters) {
			process.stderr.write(`    ${p.name} (${p.type}${p.required ? ', required' : ''}): ${p.description}\n`);
		}
	}
}

function showToolCatalog(sorted: ToolMeta[]): void {
	let currentCategory = '';
	for (const tool of sorted) {
		if (tool.category !== currentCategory) {
			if (currentCategory) process.stderr.write('\n');
			process.stderr.write(`${tool.category.toUpperCase()}\n`);
			currentCategory = tool.category;
		}
		process.stderr.write(`  ${tool.command.padEnd(32)} ${tool.description}\n`);
	}
}

const args = {
	name: {
		type: 'positional' as const,
		description: 'Tool name to show detail for (e.g. search.discover)',
		required: false,
	},
	json: {
		type: 'boolean' as const,
		description: 'Output as JSON envelope',
		default: false,
	},
} satisfies ArgsDef;

export const toolsCommand = defineCommand({
	meta: {
		name: 'tools',
		description: 'Capability discovery — list all available tools',
	},
	args,
	run({ args: parsedArgs }) {
		const start = performance.now();
		const toolName = parsedArgs.name;
		const jsonMode = isJsonMode();
		const sorted = getSortedRegistry();

		if (toolName) {
			const tool = sorted.find((t) => t.name === toolName);
			if (!tool) {
				if (jsonMode) {
					fail('tools', 'NOT_FOUND', `Tool "${toolName}" not found`, 'Run `let tools --json` to list all available tools', start);
				}
				process.stderr.write(`Tool "${toolName}" not found. Run \`let tools\` to list all.\n`);
				process.exit(1);
			}
			if (jsonMode) ok('tools', { tool }, start);
			showToolDetail(tool);
			return;
		}

		if (jsonMode) {
			ok('tools', { version: '0.0.1', tools: sorted, globalFlags: GLOBAL_FLAGS }, start, { count: sorted.length });
		}
		showToolCatalog(sorted);
	},
});
