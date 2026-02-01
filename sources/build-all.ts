/**
 * Build all data sources with parallel subprocess execution and TUI progress
 *
 * Usage: bun run sources/build-all.ts [options]
 *
 * Options:
 *   --only, -o <sources>       Comma-separated source filter
 *   --concurrency, -c <n>      Max parallel builds (default: 3)
 *   --help, -h                 Show help
 */

/* biome-ignore-all lint/suspicious/noConsole: Build orchestrator uses console for TUI output */

import { join } from 'node:path';
import { parseArgs } from 'node:util';

// ============================================================================
// Source registry
// ============================================================================

const SOURCE_NAMES = ['broadband', 'postcodes', 'deprivation', 'census', 'population', 'income', 'flood', 'naptan', 'uprn', 'crime'] as const;
type SourceName = (typeof SOURCE_NAMES)[number];

// ============================================================================
// State types
// ============================================================================

type SourceStatus = 'pending' | 'running' | 'completed' | 'failed';

interface SourceState {
	name: SourceName;
	status: SourceStatus;
	message: string;
	pct: number | null;
	elapsed: number | null;
	error: string | null;
	outputBuffer: string[];
}

// ============================================================================
// ANSI helpers
// ============================================================================

const useColor = !process.env['NO_COLOR'] && (process.env['FORCE_COLOR'] || process.stdout.isTTY);

function wrap(code: string, text: string): string {
	return useColor ? `${code}${text}\x1b[0m` : text;
}

const style = {
	bold: (t: string) => wrap('\x1b[1m', t),
	dim: (t: string) => wrap('\x1b[2m', t),
	gray: (t: string) => wrap('\x1b[90m', t),
	red: (t: string) => wrap('\x1b[31m', t),
	yellow: (t: string) => wrap('\x1b[33m', t),
	green: (t: string) => wrap('\x1b[32m', t),
};

// ============================================================================
// TUI renderer
// ============================================================================

let lastLineCount = 0;
const isTTY = process.stdout.isTTY ?? false;

function hideCursor(): void {
	if (isTTY) process.stdout.write('\x1b[?25l');
}

function showCursor(): void {
	if (isTTY) process.stdout.write('\x1b[?25h');
}

let renderScheduled = false;
let renderTimer: ReturnType<typeof setTimeout> | null = null;

function formatElapsed(ms: number): string {
	return `${(ms / 1000).toFixed(1)}s`;
}

function formatSourceLine(s: SourceState, maxNameLen: number): string {
	const name = s.name.padEnd(maxNameLen);
	const pctStr = s.pct !== null ? `${String(s.pct).padStart(3)}%` : '    ';

	switch (s.status) {
		case 'completed': {
			const elapsed = s.elapsed !== null ? formatElapsed(s.elapsed) : '';
			return `  ${style.green('\u2022')} ${style.green(name)}  ${pctStr}  ${style.dim(`completed (${elapsed})`)}`;
		}
		case 'failed':
			return `  ${style.red('\u2022')} ${style.red(name)}  ${pctStr}  ${style.red(s.error ?? 'failed')}`;
		case 'running':
			return `  ${style.yellow('\u2022')} ${style.dim(name)}  ${pctStr}  ${s.message ? style.dim(s.message) : ''}`;
		default:
			return `  ${style.gray('\u2022')} ${style.gray(name)}  ${style.gray('      pending')}`;
	}
}

function formatStatusLine(states: SourceState[], done: number): string {
	const running = states.filter((s) => s.status === 'running').length;
	const pending = states.filter((s) => s.status === 'pending').length;
	const parts: string[] = [];
	if (running > 0) parts.push(`${running} running`);
	if (done > 0) parts.push(`${done} completed`);
	if (pending > 0) parts.push(`${pending} queued`);
	return parts.join('  ');
}

function renderTUI(states: SourceState[]): void {
	if (!isTTY) return;

	const done = states.filter((s) => s.status === 'completed' || s.status === 'failed').length;
	const maxNameLen = Math.max(...states.map((s) => s.name.length));

	const lines: string[] = [style.bold(`Sources (${done}/${states.length})`), ''];
	for (const s of states) lines.push(formatSourceLine(s, maxNameLen));
	lines.push('', formatStatusLine(states, done));

	// Single-pass redraw: move cursor to start, overwrite each line
	let output = '';
	if (lastLineCount > 0) output += `\x1b[${lastLineCount}A`;
	for (const line of lines) output += `\x1b[2K${line}\n`;
	// Clear leftover lines if new output is shorter
	for (let i = lines.length; i < lastLineCount; i++) output += '\x1b[2K\n';

	process.stdout.write(output);
	lastLineCount = lines.length;
}

function scheduleRender(states: SourceState[]): void {
	if (renderScheduled) return;
	renderScheduled = true;
	renderTimer = setTimeout(() => {
		renderScheduled = false;
		renderTimer = null;
		renderTUI(states);
	}, 16);
}

function renderSummary(states: SourceState[]): void {
	const total = states.length;
	const failed = states.filter((s) => s.status === 'failed');

	console.log(style.bold(`Sources (${total}/${total})`));
	console.log('');

	const maxNameLen = Math.max(...states.map((s) => s.name.length));
	const sorted = [...states].sort((a, b) => a.name.localeCompare(b.name));

	for (const s of sorted) {
		const name = s.name.padEnd(maxNameLen);
		const elapsed = s.elapsed !== null ? formatElapsed(s.elapsed).padStart(8) : ''.padStart(8);

		if (s.status === 'completed') {
			console.log(`  ${style.green('\u2022')} ${name}  ${style.dim(elapsed)}`);
		} else {
			console.log(`  ${style.red('\u2022')} ${name}  ${style.red(s.error ?? 'failed')}`);
		}
	}

	console.log('');
	if (failed.length > 0) {
		console.log(style.red(`${failed.length} of ${total} sources failed.`));
		for (const f of failed) {
			console.log(`\n${style.red(`--- ${f.name} output ---`)}`);
			for (const line of f.outputBuffer) {
				console.log(line);
			}
		}
	} else {
		console.log(`All ${total} sources built successfully.`);
	}
}

// ============================================================================
// Subprocess execution
// ============================================================================

function processLine(state: SourceState, line: string, onUpdate: () => void): void {
	if (!line.startsWith('\x01P:')) {
		state.outputBuffer.push(line);
		return;
	}
	try {
		const data = JSON.parse(line.slice(3)) as { msg: string; pct: number | null };
		state.message = data.msg;
		if (data.pct !== null) state.pct = data.pct;
		onUpdate();
	} catch {
		state.outputBuffer.push(line);
	}
}

async function readStdout(state: SourceState, reader: ReadableStreamDefaultReader<Uint8Array>, onUpdate: () => void): Promise<void> {
	const decoder = new TextDecoder();
	let buffer = '';

	while (true) {
		const { done, value } = await reader.read();
		if (done) break;
		buffer += decoder.decode(value, { stream: true });
		const lines = buffer.split('\n');
		buffer = lines.pop() ?? '';
		for (const line of lines) processLine(state, line, onUpdate);
	}
	if (buffer) state.outputBuffer.push(buffer);
}

async function runSource(state: SourceState, onUpdate: () => void): Promise<void> {
	const scriptPath = join(import.meta.dirname, 'builders', `${state.name}.ts`);
	const startTime = performance.now();

	state.status = 'running';
	state.message = '';
	onUpdate();

	const proc = Bun.spawn(['bun', 'run', scriptPath], {
		env: { ...process.env, BUILD_PARALLEL: '1' },
		stdout: 'pipe',
		stderr: 'pipe',
	});

	await readStdout(state, proc.stdout.getReader(), onUpdate);

	const exitCode = await proc.exited;
	state.elapsed = performance.now() - startTime;

	if (exitCode === 0) {
		state.status = 'completed';
		state.pct = 100;
	} else {
		state.status = 'failed';
		const stderr = await new Response(proc.stderr).text();
		state.error = stderr.trim().split('\n').pop() ?? `exit code ${exitCode}`;
		state.outputBuffer.push(stderr);
	}
	onUpdate();
}

async function runSourceInherit(state: SourceState): Promise<void> {
	const scriptPath = join(import.meta.dirname, 'builders', `${state.name}.ts`);
	const startTime = performance.now();

	console.log('-'.repeat(60));
	console.log(`Building: ${state.name}`);
	console.log('-'.repeat(60));

	const proc = Bun.spawn(['bun', 'run', scriptPath], {
		stdout: 'inherit',
		stderr: 'inherit',
	});

	const exitCode = await proc.exited;
	state.elapsed = performance.now() - startTime;

	if (exitCode === 0) {
		state.status = 'completed';
		console.log(`\n[OK] ${state.name} completed (${formatElapsed(state.elapsed)})\n`);
	} else {
		state.status = 'failed';
		state.error = `exit code ${exitCode}`;
		console.log(`\n[FAIL] ${state.name} failed (exit ${exitCode})\n`);
	}
}

// ============================================================================
// Pool executor
// ============================================================================

async function runPool(states: SourceState[], concurrency: number): Promise<void> {
	const queue = [...states];
	const active = new Set<Promise<void>>();

	const onUpdate = () => scheduleRender(states);

	hideCursor();
	renderTUI(states);

	while (queue.length > 0 || active.size > 0) {
		while (active.size < concurrency && queue.length > 0) {
			const state = queue.shift();
			if (!state) break;
			const promise = runSource(state, onUpdate).then(() => {
				active.delete(promise);
			});
			active.add(promise);
		}

		if (active.size > 0) {
			await Promise.race(active);
		}
	}

	// Cancel any pending scheduled render
	if (renderTimer) {
		clearTimeout(renderTimer);
		renderTimer = null;
		renderScheduled = false;
	}

	// Clear TUI output before rendering final summary
	if (lastLineCount > 0) {
		let clear = `\x1b[${lastLineCount}A`;
		for (let i = 0; i < lastLineCount; i++) clear += '\x1b[2K\n';
		clear += `\x1b[${lastLineCount}A`;
		process.stdout.write(clear);
		lastLineCount = 0;
	}
	showCursor();
	renderSummary(states);
}

async function runSequential(states: SourceState[]): Promise<void> {
	console.log('='.repeat(60));
	console.log('Building data sources');
	console.log('='.repeat(60));
	console.log(`Sources: ${states.map((s) => s.name).join(', ')}\n`);

	for (const state of states) {
		await runSourceInherit(state);
	}

	console.log('='.repeat(60));
	console.log('Summary');
	console.log('='.repeat(60));

	for (const state of states) {
		const status = state.status === 'completed' ? '[OK]' : '[FAIL]';
		const elapsed = state.elapsed ? ` (${formatElapsed(state.elapsed)})` : '';
		console.log(`${status} ${state.name}${elapsed}${state.error ? `: ${state.error}` : ''}`);
	}

	const failed = states.filter((s) => s.status === 'failed');
	if (failed.length > 0) {
		console.log(`\n${failed.length} of ${states.length} sources failed.`);
	} else {
		console.log(`\nAll ${states.length} sources built successfully.`);
	}
}

// ============================================================================
// Main
// ============================================================================

async function main(): Promise<void> {
	const { values } = parseArgs({
		options: {
			only: { type: 'string', short: 'o' },
			concurrency: { type: 'string', short: 'c' },
			help: { type: 'boolean', short: 'h' },
		},
		allowPositionals: false,
	});

	if (values.help) {
		console.log(`
Build all data sources

Usage: bun run sources/build-all.ts [options]

Options:
  --only, -o <sources>       Build only specified sources (comma-separated)
  --concurrency, -c <n>      Max parallel builds (default: 3)
  --help, -h                 Show this help message

Available sources:
${SOURCE_NAMES.map((s) => `  - ${s}`).join('\n')}
`);
		return;
	}

	const selectedNames = values.only?.split(',').map((s) => s.trim().toLowerCase()) ?? [...SOURCE_NAMES];
	const concurrency = values.concurrency ? Number.parseInt(values.concurrency, 10) : 3;

	const validNames = selectedNames.filter((n): n is SourceName => (SOURCE_NAMES as readonly string[]).includes(n));
	if (validNames.length === 0) {
		console.error(`No matching sources found. Available: ${SOURCE_NAMES.join(', ')}`);
		process.exit(1);
	}

	const states: SourceState[] = validNames.map((name) => ({
		name,
		status: 'pending',
		message: '',
		pct: null,
		elapsed: null,
		error: null,
		outputBuffer: [],
	}));

	const cleanup = () => showCursor();
	process.on('SIGINT', () => {
		cleanup();
		process.exit(130);
	});
	process.on('SIGTERM', cleanup);

	if (concurrency <= 1) {
		await runSequential(states);
	} else {
		await runPool(states, concurrency);
	}

	const failed = states.filter((s) => s.status === 'failed');
	if (failed.length > 0) process.exit(1);
}

main().catch((err) => {
	showCursor();
	console.error('Build orchestrator failed:', err);
	process.exit(1);
});
