/**
 * Minimal ANSI styling utilities for consistent CLI output.
 */

type Styler = (text: string) => string;

const ANSI = {
	reset: '\x1b[0m',
	bold: '\x1b[1m',
	dim: '\x1b[2m',
	gray: '\x1b[90m',
	red: '\x1b[31m',
	yellow: '\x1b[33m',
	green: '\x1b[32m',
};

function useColor(): boolean {
	if (process.env['NO_COLOR']) return false;
	if (process.env['FORCE_COLOR']) return true;
	return Boolean(process.stdout.isTTY);
}

function wrap(code: string, text: string): string {
	if (!useColor()) return text;
	return `${code}${text}${ANSI.reset}`;
}

export const color: Record<'bold' | 'dim' | 'gray' | 'red' | 'yellow' | 'green', Styler> = {
	bold: (text) => wrap(ANSI.bold, text),
	dim: (text) => wrap(ANSI.dim, text),
	gray: (text) => wrap(ANSI.gray, text),
	red: (text) => wrap(ANSI.red, text),
	yellow: (text) => wrap(ANSI.yellow, text),
	green: (text) => wrap(ANSI.green, text),
};

export function stripAnsi(input: string): string {
	// biome-ignore lint/suspicious/noControlCharactersInRegex: ANSI escape codes
	return input.replace(/\x1b\[[0-9;]*m/g, '');
}

export function visibleLength(input: string): number {
	return stripAnsi(input).length;
}

export function getTerminalWidth(): number {
	return process.stdout.columns ?? 80;
}
