/**
 * Help command - Flattened view of all commands and options
 * Styled to match citty's built-in renderUsage()
 *
 * let help              - Show all commands
 * let help fetch        - Show only fetch options
 * let help assess       - Show only assess options
 * let help view         - Show only view subcommands
 * let help output       - Show only output subcommands
 * let help ops          - Show only ops subcommands
 */

import { log } from '@let/core/utils/logger';
import type { ArgsDef, CommandDef, CommandMeta } from 'citty';
import { defineCommand } from 'citty';
import { print, printKeyValues, section, subheader } from '../output/index.js';
import { assessCommand } from './assess.js';
import { fetchCommand } from './fetch.js';
import { opsCommand } from './ops/index.js';
import { outputCommand } from './output/index.js';
import { viewCommand } from './view/index.js';

/** Get description from command meta (handles Resolvable type) */
function getDescription(meta: CommandDef<ArgsDef>['meta']): string {
	return (meta as CommandMeta | undefined)?.description ?? '';
}

/** All top-level commands (ordered: acquire -> assess -> view -> output -> ops) */
// biome-ignore lint/suspicious/noExplicitAny: citty types don't align with exactOptionalPropertyTypes
const commands: Record<string, CommandDef<any>> = {
	fetch: fetchCommand,
	assess: assessCommand,
	view: viewCommand,
	output: outputCommand,
	ops: opsCommand,
};

/** Format columns with first column right-aligned, others left-aligned (citty style) */
function formatLineColumns(lines: string[][], linePrefix = ''): string {
	const maxLength: number[] = [];
	for (const line of lines) {
		for (const [i, element] of line.entries()) {
			maxLength[i] = Math.max(maxLength[i] ?? 0, element.length);
		}
	}
	return lines.map((l) => linePrefix + l.map((c, i) => c[i === 0 ? 'padStart' : 'padEnd'](maxLength[i] ?? 0)).join('  ')).join('\n');
}

/** Format a single argument for display (citty style) */
function formatArg(name: string, arg: ArgsDef[string]): { flag: string; desc: string } {
	if (arg.type === 'positional') {
		const bracket = arg.required ? `<${name}>` : `[${name}]`;
		return { flag: bracket, desc: arg.description ?? '' };
	}
	// Citty style: --flag="default" for strings with defaults, --flag for booleans
	if (arg.type === 'string' && arg.default !== undefined) {
		return { flag: `--${name}="${arg.default}"`, desc: arg.description ?? '' };
	}
	return { flag: `--${name}`, desc: arg.description ?? '' };
}

/** Extract option args from a command */
function getOptionArgs(cmd: CommandDef<ArgsDef>): Array<{ flag: string; desc: string }> {
	if (!cmd.args) return [];
	const options: Array<{ flag: string; desc: string }> = [];
	for (const [name, arg] of Object.entries(cmd.args)) {
		if (arg.type !== 'positional') {
			options.push(formatArg(name, arg));
		}
	}
	return options;
}

/** Add positional args to cmdLines with given prefix */
function addPositionals(cmd: CommandDef<ArgsDef>, cmdLines: string[][], prefix: string): void {
	if (!cmd.args) return;
	for (const [name, arg] of Object.entries(cmd.args)) {
		if ((arg as ArgsDef[string]).type === 'positional') {
			const formatted = formatArg(name, arg as ArgsDef[string]);
			cmdLines.push([`${prefix}${formatted.flag}`, formatted.desc]);
		}
	}
}

/** Add option args to cmdLines with given prefix */
function addOptions(cmd: CommandDef<ArgsDef>, cmdLines: string[][], prefix: string): void {
	const options = getOptionArgs(cmd);
	for (const opt of options) {
		cmdLines.push([`${prefix}${opt.flag}`, opt.desc]);
	}
}

/** Add subcommands and their args to cmdLines */
function addSubcommands(cmd: CommandDef<ArgsDef>, cmdLines: string[][], parentPrefix: string, childPrefix: string): void {
	if (!cmd.subCommands) return;
	for (const [subName, subCmd] of Object.entries(cmd.subCommands)) {
		const sub = subCmd as CommandDef<ArgsDef>;
		cmdLines.push([`${parentPrefix}${subName}`, getDescription(sub.meta)]);
		addPositionals(sub, cmdLines, childPrefix);
		addOptions(sub, cmdLines, childPrefix);
	}
}

/** Render help for a specific command family (citty style) */
function renderFamilyHelp(familyName: string, cmd: CommandDef<ArgsDef>): void {
	section(`let ${familyName}`);
	const desc = getDescription(cmd.meta);
	if (desc) {
		printKeyValues([['Description', desc]], { keyWidth: 11 });
	}

	subheader('Usage');
	print(`let ${familyName}`);

	const optionLines: string[][] = [];
	addPositionals(cmd, optionLines, '');
	addOptions(cmd, optionLines, '');
	if (optionLines.length > 0) {
		subheader('Options');
		print(formatLineColumns(optionLines, '  '));
	}

	if (cmd.subCommands) {
		const subLines: string[][] = [];
		addSubcommands(cmd, subLines, '', '  ');
		if (subLines.length > 0) {
			subheader('Subcommands');
			print(formatLineColumns(subLines, '  '));
		}
	}
}

/** Render full flattened help (citty style with nested indentation) */
function renderFullHelp(): void {
	section('Property Search Agent');
	printKeyValues(
		[
			['Version', '0.0.1'],
			['Binary', 'let'],
		],
		{ keyWidth: 7 },
	);

	const topLevelNames = [...Object.keys(commands), 'help'];
	subheader('Usage');
	print(`let ${topLevelNames.join('|')}`);

	subheader('Commands');
	const cmdLines: string[][] = [];

	for (const [name, cmd] of Object.entries(commands)) {
		cmdLines.push([name, getDescription(cmd.meta)]);
		addPositionals(cmd, cmdLines, '  ');
		addOptions(cmd, cmdLines, '  ');
		addSubcommands(cmd, cmdLines, '  ', '    ');
		cmdLines.push(['', '']);
	}

	cmdLines.push(['help', 'Show all commands (or help for specific command)']);
	cmdLines.push(['  [command]', 'Command to get help for']);

	print(formatLineColumns(cmdLines, '  '));
}

/**
 * let help [command] - Show flattened help
 */
export const helpCommand = defineCommand({
	meta: {
		name: 'help',
		description: 'Show all commands and options',
	},
	args: {
		command: {
			type: 'positional',
			description: 'Command to get help for (fetch, assess, view, output, ops)',
			required: false,
		},
	},
	run({ args }) {
		const family = args.command as string | undefined;

		if (family) {
			const cmd = commands[family];
			if (!cmd) {
				log.cli.error('Unknown command', { command: family, available: 'fetch, assess, view, output, ops' });
				process.exit(1);
			}
			renderFamilyHelp(family, cmd);
		} else {
			renderFullHelp();
		}
	},
});
