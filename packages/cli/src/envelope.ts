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
// isJsonMode
// ---------------------------------------------------------------------------

/**
 * Check if --json flag is present in process.argv.
 * Fast check, no citty dependency.
 */
export function isJsonMode(): boolean {
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
	process.stdout.write(`${JSON.stringify(envelope)}\n`);
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
	process.stdout.write(`${JSON.stringify(envelope)}\n`);
	process.exit(BLOCKING_CODES.has(code) ? 2 : 1);
}
