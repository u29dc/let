/**
 * CLI output utilities
 *
 * Unified formatting for headers, tables, and common value displays.
 */

import { color, visibleLength as visibleLen } from '@let/core/utils/terminal';
import { Table } from 'console-table-printer';

type Alignment = 'left' | 'right' | 'center';

type TableColumn = { name: string; title: string; alignment?: Alignment };

const MINIMAL_TABLE_STYLE = {
	headerTop: { left: '', mid: '', right: '', other: '' },
	headerBottom: { left: '', mid: '', right: '', other: '' },
	tableBottom: { left: '', mid: '', right: '', other: '' },
	vertical: '',
	rowSeparator: { left: '', mid: '', right: '', other: '' },
};

let hasOutput = false;
let lastWasBlank = false;

export function print(text = ''): void {
	// biome-ignore lint/suspicious/noConsole: CLI output formatting
	console.log(text);
	hasOutput = true;
	lastWasBlank = text.length === 0;
}

function ensureGap(): void {
	if (hasOutput && !lastWasBlank) {
		print('');
	}
}

export function section(title: string): void {
	ensureGap();
	print(color.bold(title));
}

export function subheader(title: string): void {
	ensureGap();
	print(color.dim(title));
}

export function gap(): void {
	ensureGap();
}

export function bold(text: string): string {
	return color.bold(text);
}

export function dim(text: string): string {
	return color.dim(text);
}

export function createTable(columns: TableColumn[]): Table {
	return new Table({ columns, style: MINIMAL_TABLE_STYLE, rowSeparator: false, shouldDisableColors: true });
}

export function formatPrice(value: number | null | undefined): string {
	if (value === null || value === undefined || Number.isNaN(value)) return '--';
	return value.toLocaleString('en-GB');
}

export function formatPercent(value: number | null | undefined): string {
	if (value === null || value === undefined || Number.isNaN(value)) return '--';
	return `${Math.round(value)}%`;
}

export function formatScore(value: number | null | undefined): string {
	return formatPercent(value);
}

export function scoreSignal(score: number | null | undefined): string {
	if (score === null || score === undefined || Number.isNaN(score)) return '--';
	const dot = '•';
	if (score >= 70) return color.green(dot);
	if (score >= 50) return color.yellow(dot);
	if (score >= 30) return color.red(dot);
	return color.gray(dot);
}

export function formatScoreWithSignal(score: number | null | undefined): string {
	const text = formatScore(score);
	if (text === '--') return '--';
	return `${text} ${scoreSignal(score)}`;
}

export function formatValue(value: string | number | null | undefined, fallback = '--'): string {
	if (value === null || value === undefined || value === '') return fallback;
	return String(value);
}

export function colorStatus(status: string | null | undefined): string {
	if (!status) return color.gray('--');
	const dot = status === 'active' ? color.green('•') : color.red('•');
	return `${status} ${dot}`;
}

export function colorRecommendation(rec: string | null | undefined): string {
	if (!rec) return color.gray('--');
	const lower = rec.toLowerCase();
	const dot = lower === 'strong-recommend' || lower === 'recommend' ? color.green('•') : lower === 'neutral' ? color.yellow('•') : color.red('•');
	return `${rec} ${dot}`;
}

export function colorQuality(quality: string | null | undefined): string {
	if (!quality) return color.gray('--');
	const dot = quality === 'excellent' || quality === 'good' ? color.green('•') : quality === 'fair' ? color.yellow('•') : color.red('•');
	return `${quality} ${dot}`;
}

/** Get visible length of string (excluding ANSI codes) */
export function visibleLength(str: string): number {
	return visibleLen(str);
}

/** Pad string to width (accounting for ANSI codes) */
export function padEnd(str: string, width: number): string {
	const visible = visibleLength(str);
	return str + ' '.repeat(Math.max(0, width - visible));
}

/** Truncate string to max width (accounting for ANSI codes) */
function truncateToWidth(str: string, maxWidth: number): string {
	const visible = visibleLength(str);
	if (visible <= maxWidth) return str;
	return `${str.slice(0, maxWidth - 1)}\u2026`;
}

export type KeyValueRow = [string, string];

type KeyValueOptions = {
	keyWidth?: number;
	gap?: number;
	indent?: number;
	dimKeys?: boolean;
};

export function printKeyValues(rows: KeyValueRow[], options: KeyValueOptions = {}): void {
	const keyWidth = options.keyWidth ?? Math.max(0, ...rows.map(([key]) => key.length));
	const gap = options.gap ?? 2;
	const indent = options.indent ?? 0;
	const dimKeys = options.dimKeys ?? true;
	const prefix = ' '.repeat(Math.max(0, indent));

	for (const [key, value] of rows) {
		const label = dimKeys ? dim(key) : key;
		const lines = value.split('\n');
		const first = lines.shift() ?? '';
		print(`${prefix}${padEnd(label, keyWidth)}${' '.repeat(gap)}${first}`);
		for (const line of lines) {
			print(`${prefix}${' '.repeat(keyWidth)}${' '.repeat(gap)}${line}`);
		}
	}
}

type ListOptions = {
	prefix?: string;
	indent?: number;
};

export function printList(items: string[], options: ListOptions = {}): void {
	const prefix = options.prefix ?? '- ';
	const indent = options.indent ?? 0;
	const indentStr = ' '.repeat(Math.max(0, indent));
	for (const item of items) {
		print(`${indentStr}${prefix}${item}`);
	}
}

/**
 * Wrap text to fit within maxWidth, preserving word boundaries.
 * Returns newline-separated string compatible with printKeyValues.
 */
export function wrapText(text: string, maxWidth: number): string {
	if (!text || maxWidth <= 0) return text;
	const words = text.split(/\s+/);
	const lines: string[] = [];
	let currentLine = '';

	for (const word of words) {
		if (currentLine.length === 0) {
			currentLine = word;
		} else if (currentLine.length + 1 + word.length <= maxWidth) {
			currentLine += ` ${word}`;
		} else {
			lines.push(currentLine);
			currentLine = word;
		}
	}
	if (currentLine.length > 0) {
		lines.push(currentLine);
	}
	return lines.join('\n');
}

type TwoColumnOptions = {
	keyWidth?: number;
	valueWidth?: number;
	gutter?: number;
	dimKeys?: boolean;
};

/**
 * Expand key-value rows into flat lines (handling multi-line values).
 * Returns array of [label, value] where label is empty string for continuation lines.
 */
function expandRows(rows: KeyValueRow[], keyWidth: number, dimKeys: boolean): [string, string][] {
	const result: [string, string][] = [];
	for (const [key, value] of rows) {
		const label = dimKeys ? dim(key) : key;
		const lines = value.split('\n');
		const first = lines.shift() ?? '';
		result.push([padEnd(label, keyWidth), first]);
		for (const line of lines) {
			result.push([' '.repeat(keyWidth), line]);
		}
	}
	return result;
}

/**
 * Print two key-value lists side by side.
 * Uses fixed column widths for consistent layout regardless of terminal width.
 */
export function printTwoColumns(left: KeyValueRow[], right: KeyValueRow[], options: TwoColumnOptions = {}): void {
	const gutter = options.gutter ?? 4;
	const keyWidth = options.keyWidth ?? 14;
	const valueWidth = options.valueWidth ?? 36;
	const dimKeys = options.dimKeys ?? true;
	const gap = 2;

	const colWidth = keyWidth + gap + valueWidth;

	const leftExpanded = expandRows(left, keyWidth, dimKeys);
	const rightExpanded = expandRows(right, keyWidth, dimKeys);

	const maxLines = Math.max(leftExpanded.length, rightExpanded.length);

	for (let i = 0; i < maxLines; i++) {
		const [lKey, lVal] = leftExpanded[i] ?? ['', ''];
		const [rKey, rVal] = rightExpanded[i] ?? ['', ''];

		// Truncate left value to prevent pushing right column
		const lValTrunc = truncateToWidth(lVal, valueWidth);
		const leftStr = lKey ? `${lKey}${' '.repeat(gap)}${lValTrunc}` : '';
		const leftPadded = padEnd(leftStr, colWidth);
		const rightStr = rKey ? `${rKey}${' '.repeat(gap)}${rVal}` : '';

		print(`${leftPadded}${' '.repeat(gutter)}${rightStr}`);
	}
}
