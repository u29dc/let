/**
 * Tool registry — single source of truth for command metadata.
 *
 * Commands are defined via defineToolCommand(), which wraps citty's
 * defineCommand() and registers tool metadata in a global array.
 * The `tools` command reads this array to produce the catalog.
 */

import { type ArgsDef, type CommandDef, defineCommand } from 'citty';

// ---------------------------------------------------------------------------
// Types
// ---------------------------------------------------------------------------

export interface ParameterMeta {
	name: string;
	type: string;
	required: boolean;
	description: string;
}

export interface ToolMeta {
	/** Dotted name e.g. "search.discover" */
	name: string;
	/** Full CLI command e.g. "let search discover" */
	command: string;
	/** Human-readable description */
	description: string;
	/** Command group e.g. "search" */
	category: string;
	/** Parameter metadata extracted from citty args */
	parameters: ParameterMeta[];
	/** Top-level fields in the JSON data payload */
	outputFields: string[];
	/** Whether repeated calls produce the same result */
	idempotent: boolean;
	/** Rate limit domain (null if none) */
	rateLimit: string | null;
	/** Example invocation */
	example: string;
}

/**
 * Extra metadata provided alongside the standard citty CommandDef.
 * description and parameters are extracted from citty meta/args.
 */
interface ToolCommandMeta {
	name: string;
	command: string;
	category: string;
	outputFields: string[];
	idempotent: boolean;
	rateLimit: string | null;
	example: string;
}

// ---------------------------------------------------------------------------
// Registry
// ---------------------------------------------------------------------------

/** Global registry populated as a side effect of defineToolCommand() */
export const toolRegistry: ToolMeta[] = [];

// ---------------------------------------------------------------------------
// Parameter extraction
// ---------------------------------------------------------------------------

function extractParametersFromArgs(args: ArgsDef | undefined): ParameterMeta[] {
	if (!args) return [];
	const params: ParameterMeta[] = [];
	for (const [key, def] of Object.entries(args)) {
		const argType = def.type ?? 'string';
		const name = argType === 'positional' ? `<${key}>` : `--${key}`;
		params.push({
			name,
			type: argType === 'positional' ? 'string' : argType,
			required: def.required ?? false,
			description: def.description ?? '',
		});
	}
	return params;
}

// ---------------------------------------------------------------------------
// defineToolCommand
// ---------------------------------------------------------------------------

/**
 * Define a CLI command with tool registry metadata.
 * Wraps citty defineCommand and registers metadata for `tools --json`.
 */
export function defineToolCommand<T extends ArgsDef = ArgsDef>(toolMeta: ToolCommandMeta, def: CommandDef<T>): CommandDef<T> {
	// Resolve args synchronously (they're always inline objects)
	const args = typeof def.args === 'function' ? undefined : (def.args as ArgsDef | undefined);
	const parameters = extractParametersFromArgs(args);

	// meta is Resolvable<CommandMeta> — we only use inline objects
	const meta = def.meta;
	const description = (meta && typeof meta === 'object' && 'description' in meta ? meta.description : undefined) ?? '';

	toolRegistry.push({
		...toolMeta,
		description,
		parameters,
	});

	return defineCommand(def);
}
