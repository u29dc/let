/**
 * JSON envelope for CLI output.
 *
 * Every command with --json outputs exactly one JSON line to stdout.
 * No other output on stdout in --json mode.
 *
 * Success: { ok: true, data: T, meta: { tool, elapsed, count?, total?, hasMore? } }
 * Error:   { ok: false, error: { code, message, hint }, meta: { tool, elapsed } }
 *
 * Exit codes: 0 = success (including partial), 1 = runtime error, 2 = prerequisites blocked
 */

// ---------------------------------------------------------------------------
// Types
// ---------------------------------------------------------------------------

interface Meta {
	tool: string;
	elapsed: number;
	count?: number | undefined;
	total?: number | undefined;
	hasMore?: boolean | undefined;
}

interface SuccessEnvelope<T> {
	ok: true;
	data: T;
	meta: Meta;
}

interface ErrorEnvelope {
	ok: false;
	error: {
		code: string;
		message: string;
		hint: string;
	};
	meta: {
		tool: string;
		elapsed: number;
	};
}

export type Envelope<T> = SuccessEnvelope<T> | ErrorEnvelope;

// Error codes that indicate blocked prerequisites (exit 2)
const BLOCKING_CODES = new Set(['NO_CONFIG', 'NO_SOURCES', 'SCHEMA_MISMATCH']);

// ---------------------------------------------------------------------------
// Capture mode (test-only)
// ---------------------------------------------------------------------------

/**
 * Thrown instead of process.exit() when capture mode is enabled.
 * Tests catch this to inspect the envelope without spawning a subprocess.
 */
export class EnvelopeCapture extends Error {
	constructor(
		public readonly envelope: string,
		public readonly exitCode: number,
	) {
		super('EnvelopeCapture');
		this.name = 'EnvelopeCapture';
	}
}

let captureModeDepth = 0;
let jsonModeOverrideDepth = 0;

/** Enable or disable capture mode. Ref-counted for concurrent test safety. */
export function setCaptureMode(enabled: boolean): void {
	captureModeDepth += enabled ? 1 : -1;
}

/** Override isJsonMode() without mutating process.argv. Ref-counted for concurrent test safety. */
export function setJsonModeOverride(enabled: boolean): void {
	jsonModeOverrideDepth += enabled ? 1 : -1;
}

/** Re-throw EnvelopeCapture in command catch blocks so it propagates to the test harness. */
export function rethrowCapture(e: unknown): void {
	if (e instanceof EnvelopeCapture) throw e;
}

// ---------------------------------------------------------------------------
// isJsonMode
// ---------------------------------------------------------------------------

/**
 * Check if --json flag is present in process.argv.
 * Fast check, no citty dependency.
 */
export function isJsonMode(): boolean {
	if (jsonModeOverrideDepth > 0) return true;
	return process.argv.includes('--json');
}

// ---------------------------------------------------------------------------
// ok
// ---------------------------------------------------------------------------

/**
 * Write a success envelope to stdout and exit 0.
 * In non-JSON mode this is a no-op (caller handles text output).
 */
export function ok<T>(tool: string, data: T, start: number, extra?: Partial<Pick<Meta, 'count' | 'total' | 'hasMore'>>): never {
	const elapsed = Math.round(performance.now() - start);
	const meta: Meta = { tool, elapsed };
	if (extra?.count !== undefined) meta.count = extra.count;
	if (extra?.total !== undefined) meta.total = extra.total;
	if (extra?.hasMore !== undefined) meta.hasMore = extra.hasMore;

	const envelope: SuccessEnvelope<T> = { ok: true, data, meta };
	const json = JSON.stringify(envelope);
	if (captureModeDepth > 0) throw new EnvelopeCapture(json, 0);
	process.stdout.write(`${json}\n`);
	process.exit(0);
}

// ---------------------------------------------------------------------------
// fail
// ---------------------------------------------------------------------------

/**
 * Write an error envelope to stdout and exit 1 (or 2 for blocking prereqs).
 */
export function fail(tool: string, code: string, message: string, hint: string, start: number): never {
	const elapsed = Math.round(performance.now() - start);
	const envelope: ErrorEnvelope = {
		ok: false,
		error: { code, message, hint },
		meta: { tool, elapsed },
	};
	const json = JSON.stringify(envelope);
	const exitCode = BLOCKING_CODES.has(code) ? 2 : 1;
	if (captureModeDepth > 0) throw new EnvelopeCapture(json, exitCode);
	process.stdout.write(`${json}\n`);
	process.exit(exitCode);
}

// ---------------------------------------------------------------------------
// emitRaw
// ---------------------------------------------------------------------------

/**
 * Emit a pre-built JSON envelope string and exit.
 * Used by commands (e.g. health) that build their own envelope.
 */
export function emitRaw(json: string, exitCode: number): never {
	if (captureModeDepth > 0) throw new EnvelopeCapture(json, exitCode);
	process.stdout.write(`${json}\n`);
	process.exit(exitCode);
}
